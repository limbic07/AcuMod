fn main() {
    if let Err(error) = acumod_lib::live_eval::run_web_search_probe() {
        eprintln!("真实联网搜索验收失败：{error}");
        std::process::exit(1);
    }
}
