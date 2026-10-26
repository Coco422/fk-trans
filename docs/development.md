# 开发

```bash
npm install
npm run tauri dev
```

检查：

```bash
npm run check
cd src-tauri && cargo check --offline && cargo test --offline
```

## 开发版的辅助功能权限

`npm run tauri dev` 直接运行 `src-tauri/target/debug/fk-trans`。macOS 按可执行文件授权，
所以它和安装好的 `fk-trans.app` 是**两个应用**：给其中一个授权，另一个照样没有权限，
系统设置里的开关看着是开的，应用却仍然报「未授权」。

「设置 → 诊断」里会显示当前运行的是哪个可执行文件，并能在 Finder 中定位它。
系统设置的列表里没有它时，点「+」，按 ⌘⇧G 粘贴路径添加。

改完权限要完全重启开发版：

```bash
pkill -f 'target/debug/fk-trans'; pkill -f 'tauri dev'; pkill -f vite
```

列表里出现重复的 fk-trans 时，删掉旧的，重新构建一次再添加当前的可执行文件。
