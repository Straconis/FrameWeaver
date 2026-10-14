// Embeds the app icon and version details into the Windows .exe.
fn main() {
    println!("cargo:rerun-if-changed=res/frameweaver.rc");
    println!("cargo:rerun-if-changed=res/frameweaver.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("res/frameweaver.rc", embed_resource::NONE);
    }
}
