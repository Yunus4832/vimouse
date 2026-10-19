# 更新记录

## 0.1.0 — 2026-10-19

Linux 首个 Rust 重写版本，主要面向 KDE Plasma Wayland、niri 及提供 layer-shell 的 Wayland 桌面；当前发行架构为 x86_64。

- evdev/uinput 模态鼠标控制：持续移动、速度切换、点击、拖选、滚动、标记与紧急释放。
- f 当前显示器 5×5 快速跳转；Tab 以当前指针为中心开启 4×4、3×3、2×2 精细定位。
- 多屏布局发现、Space 切屏和黄色透明网格；原生 Rust 浮层不依赖 Qt，字体内嵌。
- musl 静态单文件发布，内嵌权限规则、服务模板和第三方许可。
- install、init、install-service、start、stop、status 子命令；普通用户运行，初始化按需提权。
- 确定性控制器测试、隔离双屏验证及模拟授权/服务流程。

限制和已验证环境详见 README；GNOME/X11 完整浮层、Windows/macOS 后端尚未实现。真实输入、管理员配置及不同合成器的指针映射需要实机回归。
