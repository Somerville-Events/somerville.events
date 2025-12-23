use crate::models::Event;
use chrono::{DateTime, Utc};
use chrono_tz::America::New_York;

pub const COMMON_STYLES: &str = r#"
    :root {
        --link-color: light-dark(rgb(27, 50, 100),rgb(125, 148, 197));
        
        /* Button Colors - Adjusted for dark mode */
        --button-bg: light-dark(#e0e0e0, #333);
        --button-text: light-dark(#333, #eee);
        --button-shadow-light: light-dark(rgba(255, 255, 255, 0.8), rgba(255, 255, 255, 0.1));
        --button-shadow-dark: rgba(0, 0, 0, 0.1);
        --button-border: light-dark(#a0a0a0, #555);
        
        --primary-bg: #d13a26;
        --primary-text: #fff;
        --primary-shadow: #8c2415;
    }

    body {
        font-family: system-ui, sans-serif;
        max-width: 800px;
        margin: 0 auto;
        padding: 1rem;
        line-height: 1.5;
    }

    a {
        text-decoration: none;
        color: var(--link-color);
    }

    a:hover {
        text-decoration: underline;
    }

    header {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 1rem;
        flex-wrap: wrap;
        margin-bottom: 2rem;
    }

    header h1 {
        margin: 0;
        font-size: 2em; 
    }

    h1 { margin-bottom: 1rem; }
    h2 { margin-top: 2.5rem; }

    section {
        margin-bottom: 2.5rem;
    }

    article {
        padding: 1rem 0;
        border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    }

    article:first-child {
        border-top: 0;
    }

    article dl {
        margin: 0.5rem 0 0.75rem 0;
        display: grid;
        grid-template-columns: 7rem 1fr;
        gap: 0.25rem 1rem;
    }

    article dt {
        font-weight: 600;
    }
    
    article dd {
        margin: 0;
    }

    article p {
        margin: 0.75rem 0;
    }

    /* Button Styling */
    button, 
    .button, 
    input[type=file]::file-selector-button {
        display: inline-block;
        padding: 0.8rem 1.4rem;
        font-family: system-ui, sans-serif;
        font-size: 1rem;
        font-weight: 600;
        text-decoration: none;
        text-align: center;
        color: var(--button-text);
        background-color: var(--button-bg);
        border: none;
        border-radius: 4px;
        box-shadow: 
            inset 1px 1px 0px var(--button-shadow-light),
            inset -1px -1px 0px var(--button-shadow-dark),
            0 4px 0 var(--button-border),
            0 5px 8px rgba(0,0,0,0.2);
        cursor: pointer;
        transition: transform 0.1s, box-shadow 0.1s;
    }

    button:active,
    .button:active,
    input[type=file]::file-selector-button:active {
        transform: translateY(4px);
        box-shadow: 
            inset 2px 2px 5px rgba(0, 0, 0, 0.1),
            0 0 0 var(--button-border);
    }

    .button.primary, button.primary, button[type=submit] {
        background-color: var(--primary-bg);
        color: var(--primary-text);
        box-shadow: 
            inset 1px 1px 0px rgba(255, 255, 255, 0.2),
            inset -1px -1px 0px rgba(0, 0, 0, 0.2),
            0 4px 0 var(--primary-shadow),
            0 5px 8px rgba(0,0,0,0.3);
    }

    .button.primary:active, button.primary:active, button[type=submit]:active {
        box-shadow: 
            inset 2px 2px 5px rgba(0, 0, 0, 0.2),
            0 0 0 var(--primary-shadow);
    }

    .hidden {
        display: none !important;
    }
    "#;

pub fn format_datetime(dt: DateTime<Utc>) -> String {
    // Somerville, MA observes DST, so we use a real TZ database instead of a fixed offset.
    dt.with_timezone(&New_York)
        .format("%A, %B %d, %Y at %I:%M %p")
        .to_string()
}

fn percent_encode_query_value(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

pub fn render_event_html(
    event: &Event,
    is_details_view: bool,
    extra_controls: Option<&str>,
) -> String {
    let when_html = match event.end_date {
        Some(end) => format!(
            r#"<time datetime="{start_dt}">{start_label}</time> – <time datetime="{end_dt}">{end_label}</time>"#,
            start_dt = html_escape::encode_double_quoted_attribute(
                &event.start_date.with_timezone(&New_York).to_rfc3339()
            ),
            start_label = html_escape::encode_text(&format_datetime(event.start_date)),
            end_dt = html_escape::encode_double_quoted_attribute(
                &end.with_timezone(&New_York).to_rfc3339()
            ),
            end_label = html_escape::encode_text(&format_datetime(end)),
        ),
        None => format!(
            r#"<time datetime="{start_dt}">{start_label}</time>"#,
            start_dt = html_escape::encode_double_quoted_attribute(
                &event.start_date.with_timezone(&New_York).to_rfc3339()
            ),
            start_label = html_escape::encode_text(&format_datetime(event.start_date)),
        ),
    };

    let id = event.id.unwrap_or_default();
    let name = html_escape::encode_text(&event.name);
    let loc_str = event.location.as_deref().unwrap_or("");
    let location = html_escape::encode_text(loc_str);
    let description = html_escape::encode_text(&event.full_description);
    let website_html = event
        .url
        .as_ref()
        .map(|url| {
            format!(
                r#"<dt>Website</dt>
            <dd><a href="{href}" rel="noopener noreferrer" target="_blank">{label}</a></dd>"#,
                href = html_escape::encode_double_quoted_attribute(url),
                label = html_escape::encode_text(url),
            )
        })
        .unwrap_or_default();

    let title_html = if is_details_view {
        format!("<h1>{}</h1>", name)
    } else {
        format!(r#"<h3><a href="/event/{id}">{name}</a></h3>"#)
    };

    let category_html = match event.event_type.as_deref() {
        Some(category) if !category.is_empty() => {
            let category_encoded = percent_encode_query_value(category);
            format!(
                r#"<a href="/?category={category_query}">{category_label}</a>"#,
                category_query = html_escape::encode_double_quoted_attribute(&category_encoded),
                category_label = html_escape::encode_text(category)
            )
        }
        _ => "Not specified".to_string(),
    };

    let extra = extra_controls.unwrap_or("");

    format!(
        r#"
        <article>
            <div style="display: flex; justify-content: space-between; align-items: flex-start;">
                <div style="flex-grow: 1;">
                    {title_html}
                    <dl>
                        <dt>When</dt>
                        <dd>{when_html}</dd>
                        <dt>Location</dt>
                        <dd>{location}</dd>
                        <dt>Category</dt>
                        <dd>{category_html}</dd>
                        {website_html}
                    </dl>
                    <p>{description}</p>
                    <p><a href="/event/{id}.ical" class="button">Add to calendar</a></p>
                </div>
                {extra}
            </div>
        </article>
        "#
    )
}
