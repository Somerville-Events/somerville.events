use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use askama::Template;
use chrono::Utc;
use somerville_events::{
    features::{
        common::{
            get_color_for_type, get_icon_for_type, EventLocation, EventTypeLink, EventViewModel,
            SimpleEventViewModel,
        },
        upload::{SuccessTemplate, UploadTemplate},
        view::{DaySection, IndexTemplate, ShowTemplate},
    },
    models::EventType,
};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Template)]
#[template(
    source = "
<!DOCTYPE html>
<html lang='en'>
<head>
    <meta charset='utf-8'>
    <title>Storybook</title>
    <style>
        body { font-family: system-ui, sans-serif; max-width: 800px; margin: 2rem auto; padding: 0 1rem; }
        ul { list-style: none; padding: 0; }
        li { margin: 0.5rem 0; }
        a { text-decoration: none; color: #0066cc; font-size: 1.2rem; }
        a:hover { text-decoration: underline; }
    </style>
</head>
<body>
    <h1>📚 UI Storybook</h1>
    <p>Test rendering of application templates.</p>
    <ul>
        <li><a href='/upload'>Upload Page</a></li>
        <li><a href='/upload/success'>Upload Success</a></li>
        <li><a href='/view/index'>Event Index (Comprehensive)</a></li>
        <li><a href='/view/filtered'>Filtered List Examples</a></li>
        <li><a href='/view/details-gallery'>Details Gallery</a></li>
    </ul>
</body>
</html>
",
    ext = "html"
)]
struct StorybookIndexTemplate;

fn to_simple(vm: &EventViewModel) -> SimpleEventViewModel {
    SimpleEventViewModel {
        id: vm.id,
        name: vm.name.clone(),
        start_iso: vm.start_iso.clone(),
        start_formatted: vm.start_formatted.clone(),
        end_iso: vm.end_iso.clone(),
        end_formatted: vm.end_formatted.clone(),
        location: vm.location.clone(),
        event_types: vm.event_types.clone(),
        accent_color: vm.accent_color.clone(),
        accent_icon: vm.accent_icon.clone(),
        detail_url: format!("/event/{}", vm.id),
    }
}

async fn index() -> impl Responder {
    let html = StorybookIndexTemplate.render().unwrap();
    HttpResponse::Ok().content_type("text/html").body(html)
}

async fn story_upload() -> impl Responder {
    let template = UploadTemplate {
        idempotency_key: "00000000-0000-0000-0000-000000000000".to_string(),
    };
    HttpResponse::Ok()
        .content_type("text/html")
        .body(template.render().unwrap())
}

async fn story_upload_success() -> impl Responder {
    let template = SuccessTemplate;
    HttpResponse::Ok()
        .content_type("text/html")
        .body(template.render().unwrap())
}

#[derive(Default, Clone)]
struct MockEventBuilder {
    name: String,
    start_formatted: String,
    end_formatted: Option<String>,
    location: Option<EventLocation>,
    description: String,
    full_text: String,
    event_types: Vec<EventType>,
    url: Option<String>,
    age_restrictions: Option<String>,
    price: Option<f64>,
}

impl MockEventBuilder {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            start_formatted: "Fri, Oct 31 • 7:00 PM".to_string(),
            end_formatted: Some("10:00 PM".to_string()),
            location: Some(EventLocation::Structured {
                name: "The Burren".to_string(),
                address: "247 Elm St, Somerville, MA".to_string(),
                google_maps_link: "#".to_string(),
            }),
            description: "A spooky halloween event.".to_string(),
            full_text: "Join us for a spooky night of fun! Costumes encouraged.".to_string(),
            event_types: vec![EventType::Social],
            url: Some("https://example.com".to_string()),
            age_restrictions: Some("21+".to_string()),
            price: Some(15.0),
        }
    }

    fn with_types(mut self, types: Vec<EventType>) -> Self {
        self.event_types = types;
        self
    }

    fn with_location(mut self, location: EventLocation) -> Self {
        self.location = Some(location);
        self
    }

    fn without_location(mut self) -> Self {
        self.location = None;
        self
    }

    fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self.full_text = desc.to_string(); // Keep full text in sync for simple updates
        self
    }

    fn with_full_text(mut self, text: &str) -> Self {
        self.full_text = text.to_string();
        self
    }

    fn with_price(mut self, price: Option<f64>) -> Self {
        self.price = price;
        self
    }

    fn with_age(mut self, age: Option<String>) -> Self {
        self.age_restrictions = age;
        self
    }

    fn with_url(mut self, url: Option<String>) -> Self {
        self.url = url;
        self
    }

    fn with_end_time(mut self, end: Option<String>) -> Self {
        self.end_formatted = end;
        self
    }

    fn build(self, id: i64) -> EventViewModel {
        let event_types: Vec<EventTypeLink> = self
            .event_types
            .iter()
            .map(|t| EventTypeLink {
                url: format!("/view/filtered?type={}", t),
                label: t.to_string(),
                icon: get_icon_for_type(t).to_string(),
                color: get_color_for_type(t),
            })
            .collect();

        let first_type = self.event_types.first().unwrap_or(&EventType::Other);

        EventViewModel {
            id,
            name: self.name,
            start_iso: Utc::now().to_rfc3339(),
            start_formatted: self.start_formatted,
            end_iso: Utc::now().to_rfc3339(),
            end_formatted: self.end_formatted,
            location: self.location.unwrap_or(EventLocation::Unknown),
            description: self.description,
            full_text_paragraphs: self
                .full_text
                .split('\n')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            event_types,
            website_link: self.url,
            google_calendar_url: "#".to_string(),
            age_restrictions: self.age_restrictions,
            price: self.price,
            accent_color: get_color_for_type(first_type),
            accent_icon: get_icon_for_type(first_type).to_string(),
        }
    }
}

// Global state to store our mock events so detail pages can find them
struct StorybookState {
    events: Mutex<HashMap<i64, EventViewModel>>,
}

// Helper to populate state if empty
fn ensure_mock_events(data: &web::Data<StorybookState>) {
    let mut events_map = data.events.lock().unwrap();
    if !events_map.is_empty() {
        return;
    }

    let mut id_counter = 1;

    // 1. All Event Types
    let all_types = vec![
        EventType::YardSale,
        EventType::Art,
        EventType::Music,
        EventType::Dance,
        EventType::Performance,
        EventType::Food,
        EventType::PersonalService,
        EventType::Meeting,
        EventType::Government,
        EventType::Volunteer,
        EventType::Fundraiser,
        EventType::Film,
        EventType::Theater,
        EventType::Comedy,
        EventType::Literature,
        EventType::Exhibition,
        EventType::Workshop,
        EventType::Fitness,
        EventType::Market,
        EventType::Sports,
        EventType::Social,
        EventType::Holiday,
        EventType::Religious,
        EventType::ChildFriendly,
        EventType::Other,
    ];

    for t in all_types {
        let event = MockEventBuilder::new(&format!("Event Type: {}", t))
            .with_types(vec![t])
            .build(id_counter);
        events_map.insert(id_counter, event);
        id_counter += 1;
    }

    // 2. Field Variations
    let variations = vec![
        MockEventBuilder::new("Maximal Event")
            .with_description("This event has everything populated.")
            .with_full_text("Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n\nUt enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.")
            .with_price(Some(100.50))
            .with_age(Some("18+".to_string()))
            .with_url(Some("https://example.com".to_string()))
            .with_types(vec![EventType::Music, EventType::Social])
            .build(id_counter),

        MockEventBuilder::new("Minimal Event")
            .with_description("No optional fields.")
            .with_price(None)
            .with_age(None)
            .with_url(None)
            .without_location()
            .with_end_time(None)
            .with_types(vec![EventType::Other])
            .build(id_counter + 1),

        MockEventBuilder::new("Free Event")
            .with_price(Some(0.0))
            .build(id_counter + 2),

        MockEventBuilder::new("Unstructured Location")
            .with_location(EventLocation::Unstructured("Somewhere near Davis Sq".to_string()))
            .build(id_counter + 3),

        MockEventBuilder::new("Unknown Location")
            .with_location(EventLocation::Unknown)
            .build(id_counter + 4),
    ];
    id_counter += 5;

    for event in variations {
        events_map.insert(event.id, event);
    }

    // 3. String Lengths
    let text_events = vec![
        MockEventBuilder::new("Short")
            .with_description("Short desc")
            .build(id_counter),

        MockEventBuilder::new("Long Name: The Annual Somerville Giant Pumpkin Smash and Composting Extravaganza with Special Guests")
            .with_description("Normal description.")
            .build(id_counter + 1),

        MockEventBuilder::new("Long Description")
            .with_description("This is a very long description that goes on and on to test how the UI handles large blocks of text without breaking the layout. It should probably wrap nicely or be truncated if that is the design decision. We just want to make sure it doesn't overflow horizontally or look terrible.")
            .build(id_counter + 2),

        MockEventBuilder::new("Empty Description")
            .with_description("")
            .build(id_counter + 3),
    ];
    // id_counter += 4; // increment if adding more groups

    for event in text_events {
        events_map.insert(event.id, event);
    }

    // 4. Extreme Edge Cases (Long values)
    let edge_case_events = vec![
        MockEventBuilder::new("Extremely Long Address")
            .with_location(EventLocation::Structured {
                name: "The Center for Very Long Addresses".to_string(),
                address: "1234567890 This Street Name Is Intentionally Excessively Long To Test How The UI Handles Text Wrapping When The Content Exceeds The Container Width And Might Break The Layout If Not Handled Correctly, Somerville, MA 02144".to_string(),
                google_maps_link: "#".to_string(),
            })
            .build(id_counter + 4),

        MockEventBuilder::new("Extremely Long Place Name That Should Probably Be Truncated Or Wrapped Gracefully In The UI Components")
            .with_location(EventLocation::Structured {
                name: "The Super Duper Extremely Long Place Name That Goes On Forever And Ever And Ever To Test UI Resilience".to_string(),
                address: "123 Normal St, Somerville, MA".to_string(),
                google_maps_link: "#".to_string(),
            })
            .build(id_counter + 5),

        MockEventBuilder::new("Extremely Long URL")
            .with_url(Some("https://example.com/calendar/event?id=1234567890&token=abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890&utm_source=newsletter&utm_medium=email&utm_campaign=spring_sale&ref=very_long_reference_string_that_might_break_layout_if_displayed_raw".to_string()))
            .build(id_counter + 6),

        MockEventBuilder::new("All Event Types (Tag Cloud Test)")
            .with_types(vec![
                EventType::YardSale, EventType::Art, EventType::Music, EventType::Dance,
                EventType::Performance, EventType::Food, EventType::PersonalService, EventType::Meeting,
                EventType::Government, EventType::Volunteer, EventType::Fundraiser, EventType::Film,
                EventType::Theater, EventType::Comedy, EventType::Literature, EventType::Exhibition,
                EventType::Workshop, EventType::Fitness, EventType::Market, EventType::Sports,
                EventType::Social, EventType::Holiday, EventType::Religious,
                EventType::ChildFriendly, EventType::Other
            ])
            .build(id_counter + 7),

        MockEventBuilder::new("Zero Event Types")
            .with_types(vec![])
            .build(id_counter + 8),

        MockEventBuilder::new("HTML Injection Attempt")
            .with_description("This description contains <script>alert('xss')</script> and <b>bold tags</b> to test escaping.")
            .with_full_text("Full text with <div style='position:fixed;top:0;left:0;width:100%;height:100%;background:red;'>overlays</div>.")
            .build(id_counter + 9),

        MockEventBuilder::new("Unicode & Emoji Overload")
            .with_description("🎉 🎃 🦃 🎅 🎄 🎆 🎇 🧨 ✨ 🎈 🧧 🎍 🎎 🎏 🎐 🎑 🎒 🎓 🎖 🎗 🎙 🎚 🎛 🎚 🎙 🎚 🎛")
            .with_full_text("Zalgotext: T̶o̶ ̶i̶n̶v̶o̶k̶e̶ ̶t̶h̶e̶ ̶h̶i̶v̶e̶-m̶i̶n̶d̶ ̶r̶e̶p̶r̶e̶s̶e̶n̶t̶i̶n̶g̶ ̶c̶h̶a̶o̶s̶.\nIñtërnâtiônàlizætiøn\n\n(ノಠ益ಠ)ノ彡┻━┻")
            .build(id_counter + 10),
    ];

    for event in edge_case_events {
        events_map.insert(event.id, event);
    }
}

async fn story_view_index(data: web::Data<StorybookState>) -> impl Responder {
    ensure_mock_events(&data);
    let events_map = data.events.lock().unwrap();

    // Reconstruct lists from map for display (sorting by ID to keep order stable)
    let mut all_events: Vec<&EventViewModel> = events_map.values().collect();
    all_events.sort_by_key(|e| e.id);

    let mut days = Vec::new();

    // Group 1: Types (first 26 events)
    let type_events: Vec<EventViewModel> =
        all_events.iter().take(26).map(|e| (*e).clone()).collect();
    days.push(DaySection {
        day_id: "day-types".to_string(),
        date_header: "All Event Types".to_string(),
        events: type_events.iter().map(to_simple).collect(),
    });

    // Group 2: Variations (next 5)
    let variation_events: Vec<EventViewModel> = all_events
        .iter()
        .skip(26)
        .take(5)
        .map(|e| (*e).clone())
        .collect();
    days.push(DaySection {
        day_id: "day-variations".to_string(),
        date_header: "Field Variations".to_string(),
        events: variation_events.iter().map(to_simple).collect(),
    });

    // Group 3: Text (next 4)
    let text_events: Vec<EventViewModel> = all_events
        .iter()
        .skip(31)
        .take(4)
        .map(|e| (*e).clone())
        .collect();
    days.push(DaySection {
        day_id: "day-text".to_string(),
        date_header: "Text Lengths".to_string(),
        events: text_events.iter().map(to_simple).collect(),
    });

    let template = IndexTemplate {
        page_title: "Somerville Events (Storybook)".to_string(),
        filter_badge: "".to_string(),
        active_filters: vec![],
        days,
        is_past_view: false,
        all_event_types: vec![],
        all_sources: vec![],
        all_locations: vec![],
        query: Default::default(),
    };
    HttpResponse::Ok()
        .content_type("text/html")
        .body(template.render().unwrap())
}

async fn story_view_show(data: web::Data<StorybookState>, path: web::Path<i64>) -> impl Responder {
    ensure_mock_events(&data);
    let events_map = data.events.lock().unwrap();

    if let Some(event) = events_map.get(&path.into_inner()) {
        let template = ShowTemplate {
            event: event.clone(),
        };
        HttpResponse::Ok()
            .content_type("text/html")
            .body(template.render().unwrap())
    } else {
        HttpResponse::NotFound().body("Event not found in storybook")
    }
}

async fn story_view_show_default() -> impl Responder {
    let template = ShowTemplate {
        event: MockEventBuilder::new("Detailed View Example")
            .with_full_text("This is the full text view.\n\nIt supports multiple paragraphs.\n\nAnd lists all details.")
            .with_types(vec![EventType::Art, EventType::Food])
            .build(999),
    };
    HttpResponse::Ok()
        .content_type("text/html")
        .body(template.render().unwrap())
}

async fn story_view_filtered(data: web::Data<StorybookState>) -> impl Responder {
    ensure_mock_events(&data);
    let events_map = data.events.lock().unwrap();
    let all_events: Vec<&EventViewModel> = events_map.values().collect();

    // Example 1: Multi-category filter (Music + Social)
    let music_social_events: Vec<EventViewModel> = all_events
        .iter()
        .filter(|e| {
            e.event_types
                .iter()
                .any(|t| t.label == "Music" || t.label == "Social")
        })
        .map(|e| (*e).clone())
        .collect();

    let example_1 = IndexTemplate {
        page_title: "Somerville Music, Social Events".to_string(),
        filter_badge: "Music, Social".to_string(),
        active_filters: vec![
            EventTypeLink {
                url: "#".to_string(),
                label: "Music".to_string(),
                icon: get_icon_for_type(&EventType::Music).to_string(),
                color: get_color_for_type(&EventType::Music),
            },
            EventTypeLink {
                url: "#".to_string(),
                label: "Social".to_string(),
                icon: get_icon_for_type(&EventType::Social).to_string(),
                color: get_color_for_type(&EventType::Social),
            },
        ],
        days: vec![DaySection {
            day_id: "day-1".to_string(),
            date_header: "Filtered Results".to_string(),
            events: music_social_events.iter().map(to_simple).collect(),
        }],
        is_past_view: false,
        all_event_types: vec![],
        all_sources: vec![],
        all_locations: vec![],
        query: Default::default(),
    };

    // Example 2: Past Events
    let past_events: Vec<EventViewModel> =
        all_events.iter().take(3).map(|e| (*e).clone()).collect();
    let example_2 = IndexTemplate {
        page_title: "Past Somerville Events".to_string(),
        filter_badge: "".to_string(),
        active_filters: vec![],
        days: vec![DaySection {
            day_id: "day-past".to_string(),
            date_header: "Yesterday".to_string(),
            events: past_events.iter().map(to_simple).collect(),
        }],
        is_past_view: true,
        all_event_types: vec![],
        all_sources: vec![],
        all_locations: vec![],
        query: Default::default(),
    };

    let html = format!(
        "<h1>Example 1: Filtered by Music & Social</h1>{}<hr><h1>Example 2: Past Events View</h1>{}",
        example_1.render().unwrap(),
        example_2.render().unwrap()
    );

    HttpResponse::Ok().content_type("text/html").body(html)
}

async fn story_view_details_gallery(data: web::Data<StorybookState>) -> impl Responder {
    ensure_mock_events(&data);
    let events_map = data.events.lock().unwrap();

    // Select specific interesting events for the gallery
    // We sort by ID to get a predictable order:
    // 1..26 are Types
    // 27..31 are Variations (Maximal, Minimal, Free, Unstructured, Unknown)
    // 32..35 are Text Lengths (Short, Long Name, Long Desc, Empty Desc)

    // Let's pick a representative set:
    let gallery_ids = vec![
        27, // Maximal
        28, // Minimal
        30, // Unstructured Location
        31, // Unknown Location
        33, // Long Name
        34, // Long Description
        35, // Empty Description
        36, // Long Address
        37, // Long Place Name
        38, // Long URL
        39, // All Types
        40, // Zero Types
        41, // HTML Injection
        42, // Unicode/Emoji
    ];

    let mut html = String::from("<h1>Details View Gallery</h1><p>Rendering multiple detail views sequentially to verify edge cases.</p>");

    for id in gallery_ids {
        if let Some(event) = events_map.get(&id) {
            let template = ShowTemplate {
                event: event.clone(),
            };
            html.push_str(&format!("<hr><h2>Event ID {}: {}</h2>", id, event.name));
            html.push_str(&template.render().unwrap());
        }
    }

    HttpResponse::Ok().content_type("text/html").body(html)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Attempt to find static dir, default to "static"
    let static_dir = std::env::var("STATIC_FILE_DIR").unwrap_or_else(|_| "static".to_string());

    let state = web::Data::new(StorybookState {
        events: Mutex::new(HashMap::new()),
    });

    log::info!("Starting Storybook at http://localhost:8081");

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .service(actix_files::Files::new("/static", &static_dir).show_files_listing())
            .route("/", web::get().to(index))
            .route("/upload", web::get().to(story_upload))
            .route("/upload/success", web::get().to(story_upload_success))
            .route("/view/index", web::get().to(story_view_index))
            .route("/view/show", web::get().to(story_view_show_default))
            // Dynamic route for specific event details
            .route("/event/{id}", web::get().to(story_view_show))
            // Example of filtered lists
            .route("/view/filtered", web::get().to(story_view_filtered))
            // Gallery of details views
            .route(
                "/view/details-gallery",
                web::get().to(story_view_details_gallery),
            )
    })
    .bind(("127.0.0.1", 8081))?
    .run()
    .await
}
