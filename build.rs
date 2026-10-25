fn main() {
    println!("cargo:rerun-if-changed=assets/statmon.ico");
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/statmon.ico");
    res.set("ProductName", "StatMon");
    res.set("FileDescription", "StatMon - sistem izleme widget'i");
    res.set("OriginalFilename", "statmon.exe");
    if let Err(e) = res.compile() {
        println!("cargo:warning=icon kaynagi gomulemedi: {e}");
    }
}
