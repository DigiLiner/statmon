fn main() {
    println!("cargo:rerun-if-changed=assets/statmon.ico");
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/statmon.ico");
    res.set("ProductName", "StatMon");
    res.set("FileDescription", "StatMon - taskbar system monitor");
    res.set("OriginalFilename", "statmon.exe");
    res.set("FileVersion", "0.1.0.0");
    res.set("ProductVersion", "0.1.0.0");
    if let Err(e) = res.compile() {
        println!("cargo:warning=icon kaynagi gomulemedi: {e}");
    }
}
