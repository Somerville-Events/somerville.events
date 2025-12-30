# Agent Rules

## Database Migrations
Whenever you are considering adding a database migration, generate the database migration file with `sqlx migrate add <migration-name>`, then edit the resulting sql file instead of manually adding an sql file.

## Rust Dependencies
Always add rust dependencies using `cargo add` to be sure to get the latest version and be sure to visit their docs if you are uncertain how to use the latest version.

## HTML Semantics
When writing html, always use the most specific semantic element for what you're making. If you must style it, use css selector for the element, not a custom id or class unless absolutely necessary.

## Comments
Avoid commenting about WHAT the code is doing, and instead comment WHY you chose to do it that way, especially if it is unintuitive. Include how you feel about it (for instance "This is so jank") if you think it's not elegant.

## Type-safe SQLx
We should always use type-safe sqlx macros like query_as!(). We should use them offline. If the query cache is out of date, you can refresh it using sqlx.

