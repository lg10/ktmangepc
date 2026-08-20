use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // 先准备 adb 资源，确保 tauri-build / 打包阶段能看到 resources/adb
    stage_adb_resources();
    tauri_build::build();
}

/// 按编译目标把 vendor/adb 对应平台的 platform-tools 复制到 resources/adb
/// （tauri.conf.json 将其打包为包内 adb/ 目录；产物目录不入版本库）
fn stage_adb_resources() {
    let target = env::var("TARGET").unwrap_or_default();
    let src_name = if target.contains("windows") {
        "platform-tools-windows"
    } else if target.contains("darwin") {
        "platform-tools-mac"
    } else {
        "platform-tools-linux"
    };
    let src = Path::new("vendor/adb").join(src_name);
    let dst = Path::new("resources/adb");
    // 暂存平台标记放在 resources/adb 之外（不会被打包进安装包）
    let marker = Path::new("resources/.adb-staged");
    println!("cargo:rerun-if-changed=vendor/adb");

    if !src.exists() {
        println!("cargo:warning=vendor/adb/{src_name} 不存在，跳过 adb 资源准备");
        return;
    }
    // 同平台已暂存时跳过：保留外部预签名（macOS 公证要求内置二进制已签名）
    if dst.exists()
        && fs::read_to_string(marker)
            .map(|s| s.trim() == src_name)
            .unwrap_or(false)
    {
        return;
    }
    // 目标目录全量重建，避免上一平台的残留文件
    if dst.exists() {
        let _ = fs::remove_dir_all(dst);
    }
    if let Err(e) = copy_dir_recursive(&src, dst) {
        println!("cargo:warning=adb 资源复制失败: {e}");
        return;
    }
    let _ = fs::write(marker, src_name);
    #[cfg(unix)]
    {
        // 复制后确保可执行位（macOS/Linux 打包签名与运行均依赖）
        use std::os::unix::fs::PermissionsExt;
        if let Ok(entries) = fs::read_dir(dst) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
                }
            }
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
