fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/tsubame.ico");
        res.compile().expect("failed to compile Windows resources");
    }
}