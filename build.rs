fn main() {
    println!("cargo::rerun-if-changed=driver/com-exports.def");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let definition = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo provides CARGO_MANIFEST_DIR"),
        )
        .join("driver/com-exports.def");
        println!("cargo::rustc-cdylib-link-arg=/DEF:{}", definition.display());
    }
}
