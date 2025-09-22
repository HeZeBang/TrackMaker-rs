use std::process::Command;
use std::path::Path;

fn main() {
    println!("🎵 启动音频数据可视化工具...");
    
    // 切换到 tools 目录
    let tools_dir = Path::new("tools");
    if !tools_dir.exists() {
        eprintln!("❌ tools 目录不存在");
        std::process::exit(1);
    }
    
    // 检查 Python 脚本是否存在
    let script_path = tools_dir.join("sample.py");
    if !script_path.exists() {
        eprintln!("❌ sample.py 脚本不存在");
        std::process::exit(1);
    }
    
    // 运行 Python 脚本
    let mut cmd = Command::new("python3");
    cmd.current_dir(tools_dir)
        .arg("sample.py");
    
    println!("🚀 执行命令: python3 sample.py (在 tools 目录)");
    
    match cmd.status() {
        Ok(status) => {
            if status.success() {
                println!("✅ Python 脚本执行成功！");
            } else {
                eprintln!("❌ Python 脚本执行失败，退出代码: {}", status);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("❌ 无法执行 Python 脚本: {}", e);
            eprintln!("提示: 请确保已安装 Python 3");
            std::process::exit(1);
        }
    }
}
