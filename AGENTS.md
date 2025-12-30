## Project Context

- **Stack**: Rust (Actix-Web), SQLx (Postgres), Askama (Templating).
- **Key Features**: Flyer upload, OCR/LLM processing for event extraction, Geocoding.

## Database

- **Migrations**: Generate with `sqlx migrate add <migration-name>`. Edit the resulting SQL file.
- **Type-Safety**: Always use `query_as!()` or equivalent macros. Run `cargo sqlx prepare` if the offline cache is stale (CI fails).

## Code Style

- **Comments**: Focus on **WHY**, not WHAT. Be honest about "jank" code.
- **Commit message**: Say **WHAT** in under 50 chars in the title, then explain **WHY** in the body.
- **HTML**: Use semantic elements. Avoid unnecessary nesting. Avoid unnecessary custom classes/IDs. Avoid inline styles.
- **CSS**: Style element selectors directly instead of using custom classes, unless necessary.
- **Dependencies**: Use `cargo add` to ensure latest versions.
- **Javascript**: This app should work without it. Only use it for small progressive enhancements.
- **Rust**: Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` and `cargo sqlx prepare` before committing.

## Testing & Safety

- **Secrets**: Never hardcode API keys. Use `Config::from_env()`.
