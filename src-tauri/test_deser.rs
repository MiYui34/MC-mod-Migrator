use mc_mod_migrator_lib::providers::modrinth::ModrinthVersionFile;

fn main() {
    let json = r#"{"game_versions":["1.21.11"],"loaders":["fabric"],"project_id":"GcWjdA9I","version_number":"0.27.12","version_type":"release","files":[{"filename":"malilib-fabric-1.21.11-0.27.12.jar","url":"https://cdn.modrinth.com/data/x.jar","primary":true}]}"#;
    match serde_json::from_str::<ModrinthVersionFile>(json) {
        Ok(v) => println!("ok: {} {}", v.filename, v.url),
        Err(e) => println!("err: {e}"),
    }
}
