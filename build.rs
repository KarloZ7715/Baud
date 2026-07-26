fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/windows/baud.ico");
        res.compile().expect("failed to embed Windows icon resource");
    }
}
