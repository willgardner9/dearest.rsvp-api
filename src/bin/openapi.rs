//! Print the OpenAPI spec as pretty JSON.
//! This program needs no environment variable and no database.

fn main() {
    let json = dearest::openapi::spec()
        .to_pretty_json()
        .expect("The spec must convert to JSON.");
    println!("{json}");
}
