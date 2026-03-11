fn main() {
    pkg_config::Config::new()
        .atleast_version("2.0")
        .probe("kvazaar")
        .expect("kvazaar not found via pkg-config — install with `brew install kvazaar`");
}
