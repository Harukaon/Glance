# Glance

一款轻量级桌面截图翻译工具。快捷键截屏，框选即翻译，基于有道 OCR 实现实时图片翻译。

使用 Tauri 2 + 原生窗口渲染，启动快、体积小。

## 演示

![demo](assets/demo.jpg)

## 使用方法

| 操作 | 说明 |
|------|------|
| `Ctrl+Shift+X` | 全局快捷键，启动截图（可在设置中修改） |
| **鼠标左键拖拽** | 框选要翻译的区域 |
| `Esc` | 取消截图 |
| **鼠标右键** | 取消截图 |
| **点击托盘图标** | 显示主窗口 |
| **右键托盘图标** | 显示菜单（显示窗口 / 退出） |

> 关闭主窗口不会退出程序，Glance 会常驻系统托盘（右下角）。

## 功能

- 全局快捷键一键截图
- 框选区域后自动调用有道 OCR 翻译
- 翻译结果直接覆盖在原图位置
- 支持多语言互译（中/英/日/韩/法/德/俄/西）
- 翻译历史记录
- 系统托盘常驻，关闭窗口不退出
- 开机自启（可选）

## 技术栈

- **后端**：Rust + Tauri 2
- **前端**：原生 HTML / CSS / JavaScript（无框架）
- **截图**：Windows BitBlt（通过 `screenshots` crate）
- **选区窗口**：winit + softbuffer 原生渲染（无 WebView 开销）
- **翻译 API**：有道智云 OCR 图片翻译

## 项目结构

```
src-tauri/
├── src/
│   ├── main.rs            # 应用入口，托盘 & 窗口管理
│   ├── commands.rs        # Tauri 命令（截图、翻译、设置）
│   ├── capture.rs         # 屏幕截图（BitBlt）
│   ├── capture_window.rs  # 原生全屏选区窗口（winit + softbuffer）
│   ├── api.rs             # 有道翻译 API 客户端
│   ├── app_state.rs       # 全局状态管理
│   ├── config.rs          # 配置持久化
│   ├── models.rs          # 数据类型定义
│   └── error.rs           # 错误处理
├── icons/                 # 应用图标
└── Cargo.toml
ui/
├── index.html             # 主窗口
├── overlay.html           # 翻译结果浮层
├── styles.css             # 样式
└── app.js                 # 前端逻辑
```

## 开发

需要 Rust 工具链和 Tauri CLI：

```bash
cargo install tauri-cli --version "^2"
```

开发运行：

```bash
cargo tauri dev
```

打包：

```bash
cargo tauri build
```

产物在 `src-tauri/target/release/bundle/` 下。

## macOS 分发说明

Glance 当前采用 **ad-hoc 签名**，不走 App Store，也不依赖 Apple Developer 付费证书。

这意味着：

- 应用在 macOS 上应当可以正常安装和运行
- 首次从浏览器、聊天工具或网盘下载后，系统仍可能提示“无法验证开发者”或要求到 **系统设置 > 隐私与安全性** 里手动允许
- 如果系统仍提示“已损坏”，通常是下载链路附带了 quarantine 标记或压缩/传输过程破坏了应用包，可在终端执行：

```bash
xattr -dr com.apple.quarantine /Applications/Glance.app
```

更稳妥的首次打开方式：

1. 将 `Glance.app` 拖入 `Applications`
2. 在访达中右键 `Glance.app`
3. 选择“打开”
4. 若仍被拦截，到 **系统设置 > 隐私与安全性** 中点“仍要打开”

## 许可证

MIT

## 友链

- [Linux.do](https://linux.do) — 一个真实、自由、纯粹的技术社区
