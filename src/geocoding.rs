use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GooglePlacesSearchRequest<'a> {
    text_query: &'a str,
    location_bias: LocationBias,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LocationBias {
    circle: Circle,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Circle {
    center: LatLng,
    radius: i64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LatLng {
    latitude: f64,
    longitude: f64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GooglePlacesResponse {
    places: Option<Vec<GooglePlace>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GooglePlace {
    id: String,
    display_name: LocalizedText,
    formatted_address: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LocalizedText {
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeocodedLocation {
    pub formatted_address: String,
    pub place_id: String,
    pub name: String,
}

// Roughly the center of cambridge + somerville combined,
// plus a search radius wide enough to include some neighboring
// towns just in case.
// Center: 42.383971, -71.108600
// Radius: 10 miles ~= 16100 meters
const CAMBERVILLE_CENTER_LAT: f64 = 42.383971;
const CAMBERVILLE_CENTER_LON: f64 = -71.108600;
const EVENT_RADIUS_METERS: i64 = 16100;

pub async fn canonicalize_address(
    client: &awc::Client,
    location: &str,
    api_key: &str,
) -> Result<Option<GeocodedLocation>> {
    let request_body = GooglePlacesSearchRequest {
        text_query: location,
        location_bias: LocationBias {
            circle: Circle {
                center: LatLng {
                    latitude: CAMBERVILLE_CENTER_LAT,
                    longitude: CAMBERVILLE_CENTER_LON,
                },
                radius: EVENT_RADIUS_METERS,
            },
        },
    };

    let mut response = client
        .post("https://places.googleapis.com/v1/places:searchText")
        .insert_header(("X-Goog-Api-Key", api_key))
        .insert_header((
            "X-Goog-FieldMask",
            "places.id,places.displayName,places.formattedAddress",
        ))
        .send_json(&request_body)
        .await
        .map_err(|e| anyhow::anyhow!("Geocoding request failed: {}", e))?;

    if !response.status().is_success() {
        let body_bytes = response.body().await.unwrap_or_default();
        let body_str = String::from_utf8_lossy(&body_bytes);
        return Err(anyhow::anyhow!(
            "Geocoding API returned status: {} - Body: {}",
            response.status(),
            body_str
        ));
    }

    let body: GooglePlacesResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse geocoding response: {}", e))?;

    Ok(body.places.and_then(|places| {
        places.into_iter().next().map(|p| GeocodedLocation {
            formatted_address: p.formatted_address,
            place_id: p.id,
            name: p.display_name.text,
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn get_client() -> awc::Client {
        awc::ClientBuilder::new()
            .timeout(Duration::from_secs(10))
            .finish()
    }

    fn get_api_key() -> String {
        crate::config::Config::from_env()
            .google_maps_api_key
            .clone()
    }

    #[actix_rt::test]
    async fn test_canonicalize_davis_square() {
        let key = get_api_key();

        let client = get_client();
        // "Davis Square" is ambiguous globally, but with our heuristic it should find the one in Somerville, MA.
        let result = canonicalize_address(&client, "Davis Square", &key)
            .await
            .unwrap();

        assert_eq!(
            result,
            Some(GeocodedLocation {
                formatted_address: "Davis Square, Somerville, MA, USA".to_string(),
                place_id: "ChIJV1wE6Bh344kRUrVbHX8CkaM".to_string(),
                name: "Davis Square".to_string(),
            })
        );
    }

    #[actix_rt::test]
    async fn test_canonicalize_somerville_theatre() {
        let key = get_api_key();

        let client = get_client();
        let result = canonicalize_address(&client, "Somerville Theater", &key)
            .await
            .unwrap();

        assert_eq!(
            result,
            Some(GeocodedLocation {
                formatted_address: "55 Davis Square, Somerville, MA 02144, USA".to_string(),
                place_id: "ChIJoeqWSh9344kRe2ICgJs6oEQ".to_string(),
                name: "Somerville Theatre".to_string(),
            })
        );
    }

    #[actix_rt::test]
    async fn test_canonicalize_partial_address() {
        let key = get_api_key();

        let client = get_client();
        // "123 Highland Ave" is common. With "Somerville, MA" appended, it should find the one in Somerville.
        let result = canonicalize_address(&client, "123 Highland Ave, Somerville", &key)
            .await
            .unwrap();

        assert_eq!(
            result,
            Some(GeocodedLocation {
                formatted_address: "123 Highland Ave, Somerville, MA 02143, USA".to_string(),
                place_id: "ChIJIdDVfTJ344kRmPCDDrc_KuE".to_string(),
                name: "123 Highland Ave".to_string(),
            })
        );
    }

    #[actix_rt::test]
    async fn test_canonicalize_explicit_address() {
        let key = get_api_key();

        let client = get_client();
        // If we give it a full address, it should respect it and maybe just format it nicer.
        let input = "93 Highland Ave, Somerville, MA 02143";
        let result = canonicalize_address(&client, input, &key).await.unwrap();

        assert_eq!(
            result,
            Some(GeocodedLocation {
                formatted_address: "93 Highland Ave, Somerville, MA 02143, USA".to_string(),
                place_id: "ChIJY2HZpDJ344kRHPpJQ-wMcRw".to_string(),
                name: "93 Highland Ave".to_string(),
            })
        );
    }

    #[actix_rt::test]
    async fn test_canonicalize_unknown_place() {
        let key = get_api_key();

        let client = get_client();
        let result = canonicalize_address(&client, "ThisPlaceDefinitelyDoesNotExist12345", &key)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[actix_rt::test]
    async fn test_canonicalize_pumpkin_smash() {
        let key = get_api_key();

        let client = get_client();
        let result = canonicalize_address(
            &client,
            "Somerville Community Growing Center, 22 Vinal Ave",
            &key,
        )
        .await
        .unwrap();
        assert_eq!(
            result,
            Some(GeocodedLocation {
                formatted_address: "22 Vinal Ave, Somerville, MA 02143, USA".to_string(),
                place_id: "ChIJqY2aUDN344kRMn87E8bG4ZY".to_string(),
                name: "Somerville Community Growing Center".to_string()
            })
        );
    }
}
