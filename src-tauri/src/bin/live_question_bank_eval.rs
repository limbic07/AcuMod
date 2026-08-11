fn main() {
    if let Err(error) = acumod_lib::live_eval::run() {
        eprintln!("真实题库验收失败：{error}");
        std::process::exit(1);
    }
}
