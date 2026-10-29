# 使用与开发指南

日常安装与基本操作见 [README](../README.md)。本文提供完整按键、权限配置、持久标记与开发说明。涉及源码路径的命令均在项目根目录执行。

## 安装与设备权限

下载静态二进制后，在桌面终端中以普通用户执行：

```sh
chmod +x ./vimouse
./vimouse install
./vimouse init
./vimouse doctor
./vimouse run
```

`install` 只将当前二进制原子复制到用户安装目录 `~/.local/bin/vimouse`，无需管理员权限，不配置权限、不安装或启用服务。可以重复执行以更新程序；若该目录不在 PATH，命令会提示添加。`init` 不包含程序安装；`install-service` 会自动包含相同的程序安装步骤，因此使用服务时不必先执行 `install`。

`init` 会通过 sudo（未安装时使用 pkexec）请求一次管理员认证，自动检测 uinput、加载需要的内核模块、创建 `vimouse` 用户组、添加当前账户、写入 udev 和模块加载规则，并直接设置现有设备的 ACL。规则和 ACL 编码均内嵌，不需要安装包、配置模板、`acl` 软件包或 `setfacl`。日常运行始终以普通用户进行。

当前已连接设备的权限立即生效；注销并重新登录一次，让新用户组也覆盖后续热插拔设备和用户服务。若内核不支持设备 ACL，程序会提示使用注销登录后的组权限。初始化可以重复运行；不安装或启动服务，不改动现有用户配置。

权限配置完成后，只需这个 `vimouse` 可执行文件。原生浮层要求桌面支持 `wlr-layer-shell`（如 KDE Plasma、niri）；启动前检查实际协议，不按桌面名称猜测。X11/GNOME 暂无原生浮层后端，可使用 `--no-hud` 运行输入控制器。

| 阶段 | 所需系统能力 |
| --- | --- |
| 日常运行 | Linux evdev/uinput；浮层需要支持 layer-shell 的 Wayland 桌面 |
| 一次性 `init` | sudo 或 pkexec；groupadd、usermod、udevadm；需要加载模块时使用 modprobe |
| 可选用户服务 | systemd 用户会话与 systemctl |

显示器布局可以直接从 Wayland 读取；KDE 上已有的 `kscreen-doctor`、`qdbus6` 仍可用于桌面专用查询，但不是必装依赖。未安装 `qdbus6` 时无法查询 KDE 当前活动输出，仍可使用 Space 切屏；niri 通过内嵌 IPC 客户端查询。

发行压缩包 `vimouse-linux-x86_64.tar.gz` 仅包含 `vimouse-linux-x86_64/vimouse`，解压后直接执行 `./vimouse init` 和 `./vimouse install`。权限规则、服务模板和项目/第三方许可证均内嵌；使用 `vimouse licenses` 查看。源码目录中的 `packaging/install.sh` 仅保留为兼容入口，不再放入发行包。

`init` 会先通过 `UI_GET_VERSION` 验证内核接口；已有 uinput 可用时不再调用 modprobe。仅有 `/dev/uinput` 文件并不能证明驱动可用。如果运行内核与 `/lib/modules/$(uname -r)` 不匹配且接口不可用，安装会在设备权限配置前停止；内核升级后请保存工作并重启到已安装内核，再重新执行 `vimouse init`。在容器/chroot 中则需要主机提供 uinput 设备。可单独执行 `sudo ./vimouse check-uinput` 做只读接口检查，不创建虚拟设备。

## 安装用户服务与自启动

权限初始化并测试成功后，直接通过二进制管理服务：

```sh
./vimouse install-service
./vimouse start
./vimouse status
./vimouse stop
```

- `install-service` 将当前可执行文件原子安装到 `~/.local/bin/vimouse`，写入用户服务并启用图形登录自启动；本次不启动。可重复运行以更新程序，已有配置保留。
- `start` 启动 `vimouse.service`，等待浮层和输入控制器就绪；失败时返回错误。命令可重复执行；已有实例不会重复启动。更新正在运行的程序后，用 `stop` 再 `start` 加载新版本。
- `stop` 停止服务及其浮层、释放键盘；下次登录仍会自动启动。取消登录自启动使用 `systemctl --user disable vimouse.service`。
- `status` 显示服务状态。日志使用 `journalctl --user -u vimouse.service -n 40`。

四个命令都以普通桌面用户执行，不使用 sudo。控制器和浮层由同一个服务管理；安装时会停用旧版独立 `vimouse-hud.service`，避免重复浮层。不要同时手动运行 `run` 和服务。

在桌面终端执行 `install-service` / `start` 时，程序只将显示连接变量写入当前登录会话的临时单元配置（`$XDG_RUNTIME_DIR/systemd/user/vimouse.service.d/session.conf`），不导入其他 shell 变量、不改变其他服务环境。后续图形登录通常由桌面向 systemd 用户会话提供显示变量；niri 应使用它的 systemd 会话启动方式。服务随图形会话启停，不需要启用 linger。

## 接管真实鼠标

```sh
./target/release/vimouse doctor
./target/release/vimouse list-devices
./target/release/vimouse run
```

默认捕获启动时可读的完整键盘，排除 Vimouse 自己创建的虚拟设备。可显式选择键盘，推荐使用稳定的 `/dev/input/by-id/*-event-kbd` 路径：

```sh
./target/release/vimouse run --device /dev/input/by-id/YOUR-KEYBOARD-event-kbd
```

`--device` 可以重复。启动失败或正常退出会释放已经捕获的设备；运行期间若选中的键盘断开，程序停止并释放控制，重新启动即可连接。首版不自动接管热插拔键盘。

### 输入设备权限

后端通过 Linux evdev 读取并独占键盘，通过 uinput 创建虚拟键盘和绝对坐标指针，不依赖 XWayland。需要可读写的 `/dev/uinput` 和所选键盘的 event 节点。`doctor` 只检查访问条件，不捕获设备，也不创建虚拟设备。

推荐直接执行 `./target/release/vimouse init` 自动配置。以下是等效的永久权限手动配置，便于审阅：

```sh
sudo modprobe uinput
sudo groupadd -f vimouse
sudo usermod -aG vimouse "$USER"
sudo install -m 0644 packaging/70-vimouse.rules /etc/udev/rules.d/70-vimouse.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=input
sudo udevadm trigger --subsystem-match=misc
```

注销并重新登录，使用户组生效，再运行 `doctor`。需要开机加载模块时，可将 `uinput` 写入 `/etc/modules-load.d/vimouse.conf`。如果发行版或设备有额外访问限制，以 `doctor` 输出为准。

`vimouse` 组成员能够读取键盘输入并注入系统输入，只应将自己的账户加入。程序按普通用户运行；浮层也使用该用户的桌面会话。

## 操作

程序从 **Insert** 模式启动，普通按键透传。默认 `Caps Lock` 切换鼠标控制与输入；在 Insert 中快速连按两次（350ms 内）切换原生大小写锁定并留在 Insert。可以在配置中将切换键换成 F8–F12。

| 按键 | 行为 |
| --- | --- |
| `h j k l` | 按住持续左、下、上、右移动；支持对角移动 |
| Normal 中 `Space` | 独立切换到下一块显示器，鼠标居中；无需进入 f |
| `f` | 5×5 快速选块，选中字母后跳到中心并立即回到 Normal |
| Normal 中 `Tab` | 以当前指针为中心展开 4×4 精细网格 |
| Normal 中 `1` / `2` / `3` | 直接选择快 / 中 / 慢速 |
| Normal 中 `M`（Shift+m） | 在所有连接的显示器上显示 mark，按字母跳转，Esc 或 M 收起 |
| Normal 中 `?`（Shift+/） | 居中显示快捷键帮助，Esc、? 或 Enter 收起 |
| `a` / `Alt+a` / `Ctrl+a` | 循环三档速度 / 最快 / 最精细 |
| `Enter` / `Alt+Enter` / `Ctrl+Enter` | 左键 / 右键并进入 Select / 中键 |
| `Alt+h j k l` | 水平或垂直滚动 |
| `Ctrl+b` / `Ctrl+f` | 向上 / 向下持续滚动 |
| `i` / `Shift+i` | 进入 Insert / 左键点击后进入 Insert |
| `v` / `Ctrl+v` | 开始左键拖选 / 中键拖动；再次按下释放 |
| `m` 后接 `a`–`z` | 保存当前位置为标记 |
| `'` 或反引号，后接标记名 | 跳转到标记 |
| `''` / 两个反引号 | 与上次跳转位置交换 |
| `gg` / `gc` / `ge` | 左上角 / 中心 / 右下角 |
| `zt` 或 `z Enter` / `zz` / `zb` | 垂直顶部 / 中间 / 底部，保留横坐标 |
| `s` | 进入 Select，`hjkl` 映射方向键 |
| Select 中 `Enter` / `q` 或 `Esc` / `Shift+q` | 确认 / 发送 Esc 并返回 Normal / 直接返回 Normal |
| `Alt+Tab` / `Ctrl+Tab` | 原生窗口 / 标签切换 |
| `:` | 输入命令；Backspace 编辑，Esc 取消 |
| `:q Enter` / `:quit Enter` | 退出程序 |
| `:marks Enter` / `:help Enter` | 打开 mark 视图 / 居中帮助 |
| `Esc` | 取消当前模式、释放拖选、回到 Normal |
| `Ctrl+Alt+Esc` | 从任意模式返回 Insert，释放 Vimouse 持有的输入 |
| `Ctrl+Alt+Backspace` | 从任意模式退出程序 |

鼠标控制上下文会消费未映射的按键。命令采用物理 US 键位，不经过输入法；Insert 的输入仍按系统布局和输入法处理。60 秒无活动会返回 Insert，可配置；持续移动时不会触发超时。更改模式、退出、SIGINT/SIGTERM 和常规错误路径都会释放拖选及已转发的按键。

### 多显示器

程序启动时自动读取 KDE 的 KScreen 或 niri IPC，使用各显示器缩放、旋转后的逻辑尺寸和桌面偏移。普通模式空格按显示器布局从左到右、同列从上到下循环切屏，同时把鼠标移到目标显示器中心；空格不会进入 f 模式。Insert 的空格仍正常输入；精细定位工具内空格完成定位。f 选块会自动完成，无需再按空格。

`f` 在当前活动显示器上建立网格，不跨整个桌面划分。进入普通模式及 f 时，会从合成器读取活动输出。每块物理屏幕拥有独立的网格窗口，只显示当前输出对应的窗口，避免移动 layer-shell 窗口引起错屏。支持不同缩放、负坐标和非水平排列；绝对鼠标事件按整个桌面统一归一化。

可运行 `cargo run -- displays` 查看逻辑布局、活动输出和识别来源。布局可直接从 Wayland 读取；KDE 的 `kscreen-doctor` 和 `qdbus6` 是可选查询工具，其中 `qdbus6` 用于查询当前活动显示器。niri 通过 NIRI_SOCKET 读取。无法识别布局的环境使用配置文件里的单屏尺寸。静止时约每三秒重新检查布局；布局改变会结束正在进行的网格操作并清空临时跳转历史；持久 mark 保留，按显示器名称和相对位置重新解析。

### f 快速选块

Normal 中按 `f`，只将当前显示器切分为 5×5，字母按行排列：

```text
a b c d e
f g h i j
k l m n o
p q r s t
u v w x y
```

按 `a`–`y` 后，鼠标移动到对应区块中心，网格立即结束，回到 Normal；不点击、不继续缩小，也不需要确认键。接下来可以直接按住 hjkl 连续移动，或按 Enter 点击。Esc 取消并保留进入前的位置。

f 是高频的单次跳转工具；选格后已在 Normal，空格将执行独立切屏功能。

### Tab 精细定位

Normal 中按 `Tab`，以 Vimouse 当前指针坐标为中心，重新展开 4×4 网格。区域宽、高均为当前显示器逻辑宽、高的 1/5，位置不受 f 的区块边界限制。进入本身不移动鼠标，也不要求之前使用过 f。

每次打开 Tab 都按当前指针位置重新计算区域；靠近屏幕边缘时，平移整个区域使其完全留在当前显示器，保持尺寸不变，鼠标仍不跳动。选中字母后进入 3×3 微调，再按 Tab 可细分到 2×2。

- `a`–`p`：选择 4×4 子区块，鼠标移到中心，进入无字母的 3×3 微调。选择后仍在精细工具内。
- `h/j/k/l`：在 3×3 或 2×2 中移动选中格，边缘停止；其他字母（除回退键 z）无动作。
- 精细阶段的 `Tab`：将 3×3 当前选中格继续细分为 2×2，到此停止。
- `z` 或 Backspace：返回上一层并恢复选中位置；回到初始 4×4 时恢复 Tab 进入前的坐标，不重新打开 f。
- `Space`：完成当前精细定位，回到 Normal，保留位置、不点击。
- `Enter`：完成并左键点击；Ctrl+Enter 中键，Alt+Enter 右键并进入 Select。
- `Esc`：取消本次精细定位，回到 Normal 并恢复 Tab 进入时的位置。
- Caps / 紧急组合键 / 超时：退出工具，按原规则切换模式。

例如：`f → a` 完成快速跳转；接着 `hjkl` 自由移动，按 `Tab → p` 进入微调，再用 `hjkl → Tab → hjkl → Space` 完成。Ctrl+Tab、Alt+Tab 保留原生切换行为，Insert 中 Tab 正常透传。

1920×1080 下，f 的大区块是 384×216；Tab 的 4×4 子区块是 96×54，随后 3×3 每格 32×18，2×2 每格约 16×9。输入按物理按键单次触发，长按不会连续跳过层级。

两个工具共用黄色细网格。f 和 Tab 的字母位于每格中心，与选中后的鼠标落点一致；按字形可见区域居中，字号 13px，只有细描边，无底框。3×3、2×2 不显示字母，仅以细框标记当前格。全程不压暗屏幕，工具内隐藏状态胶囊和命令条。

透明窗口保持映射，只切换绘制内容，不触发开关窗口动画；始终鼠标穿透、不接收键盘焦点、不占用桌面布局。每次 f 跳转或 Tab 完成都独立保留一个跳转起点，可用 `''` 返回。旧配置的 `[grid]` 字段仍可读取但不影响新流程。

可先运行 `cargo run -- demo`，输入 `caps f a` 体验快速跳转，再输入 `l:100 tab p h space` 查看精细工具。演示不移动实体鼠标。真实控制使用普通用户运行 `cargo run -- run`。

## mark 视图与快捷键帮助

普通模式按 `M`（Shift+m），所有已连接显示器上的 mark 都会以黄色十字标出准确落点，旁边是字母标签；重合位置合并显示字母，边缘标签保持在屏内。按对应 `a`–`z` 立即跳转并回到 Normal，不点击。Esc 或再次 M 收起，不改变位置。M 模式只显示落点和字母，不显示说明面板或状态胶囊；离线标记不绘制、不能跳转，记录继续保留。`:marks Enter` 打开同一视图。

![快捷键帮助](help.png)

普通模式按 `?`（Shift+/）打开当前显示器中央的帮助卡片，仅显示已经实现的快捷键，不压暗整屏、不接收鼠标或键盘焦点；Esc、再次 ? 或 Enter 关闭。`:help Enter` 打开同一帮助。

`1`、`2`、`3` 在普通模式分别直接选择 FAST、MED、FINE，原来的 a 循环调速继续可用。命令模式中的数字仍是输入文本，Insert 中正常透传；选标记、帮助和网格模式不将数字解释为调速。

## mark 持久保存

`m{a-z}` 保存的命名 mark 会立即交给后台线程保存到 `$XDG_STATE_HOME/vimouse/marks.json`；未设置时为 `~/.local/state/vimouse/marks.json`。启动自动恢复，无需额外配置；`''` 的上一次跳转位置不写入文件。

```json
{
  "version": 1,
  "marks": {
    "a": { "display": "DP-1", "x": 0.5, "y": 0.8 }
  }
}
```

坐标来自显示器的**逻辑坐标**，文件保存屏内相对位置 0–1，不保存物理像素或桌面绝对坐标。0 和 1 分别对应首末像素；恢复时按当前逻辑尺寸换算并加上显示器偏移。缩放、分辨率和屏幕排列变化后，mark 会定位到相同的相对区域。显示器断开时保留 mark，跳转不改变位置并在状态消息中提示显示器不可用；重新接回后可继续使用。显示器名称变化不会猜测映射到其他屏幕。

状态目录权限为 0700、文件为 0600，写入使用临时文件、fsync 和原子替换；只有新增或覆盖 mark 时写入。损坏或不支持版本的文件会在捕获键盘前报错，并保留原文件；可修复或备份移走后启动。保存线程合并尚未写入的连续修改，磁盘同步不阻塞输入处理；正常退出先释放输入设备，再等待最新记录保存完成。运行中写入失败则保留内存记录并记录日志，普通状态胶囊以红色 `!` 提示，命令条和帮助卡片也显示保存失败；提示跨模式保留，下一次设置 mark 时重试，最新保存成功后清除。旧版本进程里的内存 mark 无法自动迁移，需要重新设置一次。

`demo` 始终使用独立的内存 mark，不读取或修改真实持久状态。手动编辑状态文件请先停止服务。

## 配置

```sh
mkdir -p ~/.config/vimouse
cp config.example.toml ~/.config/vimouse/config.toml
```

默认读取 `$XDG_CONFIG_HOME/vimouse/config.toml`，未设置时读取 `~/.config/vimouse/config.toml`。也可指定 `--config PATH`。无配置文件时使用默认值；未知字段和越界参数会在捕获输入前报错。修改后重启生效。

`speeds` 是从快到慢的三个速度。KDE/niri 自动检测显示器时采用检测到的逻辑尺寸；`screen` 的 1920×1080 默认值只用于无法检测布局时的单屏回退。`vimouse config` 可以打印完整默认配置。

状态和锁默认放在 `/tmp/vimouse-UID/`，目录权限为 0700。控制器和独立浮层可以通过相同的 `--runtime-dir DIR` 通信。浮层不接收焦点或鼠标点击；Insert 和网格模式隐藏状态 UI。Normal、Visual、Select 只显示右下角 156×30 的模式/速度胶囊；Command 展开完整输入条，M 和 ? 仅按需显示各自视图。两者距底边 64 个逻辑像素，胶囊距右边 16 个逻辑像素；切换无动画。

## 0.1.0 支持环境与限制

**完整界面面向 Linux + 支持 `wlr-layer-shell` 的 Wayland 桌面。当前发布的静态二进制架构是 x86_64。** 不能将它理解为支持所有 Wayland 桌面。

| 环境 | 0.1.0 范围与验证情况 |
| --- | --- |
| KDE Plasma Wayland | 主要目标；本机双屏逻辑布局读取、隔离 KWin 双屏原生浮层与模式切换已验证；真实输入仍需实机回归 |
| niri | 主要目标；已有用户输入及网格交互测试反馈，本次 Rust 原生浮层替换后仍需实机回归 |
| 其他提供 layer-shell 的 Wayland 合成器 | 按协议检测，可尝试；尚未逐一验证，绝对指针映射可能有差异 |
| GNOME Wayland、X11 | 暂无浮层后端；仅可尝试 `--no-hud` 输入控制，不提供完整网格体验 |
| Windows、macOS | 尚未实现平台后端 |

- 需要 Linux evdev/uinput 设备及对应权限；浮层不依赖 Qt、GTK、XWayland 或系统字体。GNU 源码构建依赖系统 libc，musl 发布版无动态库依赖；其他 CPU 架构尚未验证。
- `init` 需要 sudo/pkexec 和系统设备管理工具；`install` 无需管理员权限。服务管理命令需要 systemd 用户会话；前台 `run` 不要求通过 systemd 启动。
- 自动化验证包括状态机、CLI、临时文件 ACL 编码、模拟授权和服务管理、隔离 KWin 双屏浮层。真实键盘捕获、uinput 跨屏落点及管理员初始化仍需要实机验证。
- 多屏布局支持 Wayland xdg-output、KDE 工具和 niri IPC；布局变化在空闲时检查。KDE 当前活动输出查询依赖可选的 `qdbus6`；缺失时可用 Space 选屏。
- Wayland 不提供通用的全局指针位置查询。Vimouse 维护自己的指针坐标，实体鼠标不会直接更新保存的坐标；当前显示器查询取决于合成器支持。
- 键盘断开后释放控制并退出，重新连接后需重启；不自动接管热插拔键盘。命名标记持久保存，临时跳转历史 `''` 仍只保存在内存；按键映射暂为固定设计，速度、切换键和超时可配置。

## 从源码构建与试用

需要 Linux 和 Rust 工具链。构建后可直接复制单个可执行文件，无需携带 UI 文件。默认 GNU 构建仍依赖系统 libc；需要静态可移植版本时使用下文 musl 构建。

```sh
cargo build --release --locked
./target/release/vimouse --help
./target/release/vimouse demo --no-hud
```

演示模式**不拦截键盘、不注入输入**。在终端输入下面这一行，再按 Enter，即可运行真正的控制状态机，查看位置和动作输出：

```text
caps l:500 a j:200 m a g g ' a v l:200 v : q enter
```

`l:500` 表示按住 `l` 500 毫秒；`alt+enter` 表示组合键；独立的 `:` 进入命令模式。演示时间随这些 token 推进，`wait:60000` 可测试超时。Ctrl+D 退出演示。

```sh
# 运行完整演示脚本；去掉 --fast 后按真实时间播放
./target/release/vimouse demo --no-hud --fast < docs/demo.txt

# 查看浮层，不控制输入；Ctrl+C 关闭
./target/release/vimouse hud --preview

# 带实时浮层的演示
./target/release/vimouse demo
```

原生浮层不加载外部 GUI 运行库。`hud --preview` 仅显示速度胶囊；`demo` 可验证 f / Tab 网格，不捕获或注入系统输入。

```sh
# 静态 Linux x86_64 二进制（构建机只需安装一次 musl Rust 标准库）
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl
./target/x86_64-unknown-linux-musl/release/vimouse doctor
```

musl 版本不依赖目标系统的 glibc、libwayland 或字体库；仍需 Linux 内核接口、支持 layer-shell 的 Wayland 桌面及设备权限。单个二进制不能替代这些系统能力。

## 开发与验证

```sh
cargo fmt --all --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 scripts/check-native-hud.py # 开发用隔离 KWin 双屏，不修改桌面、不接管输入
python3 scripts/check-installer.py # 模拟 sudo/pkexec/systemctl，不修改系统权限或服务
# 构建仅含单个可执行文件的发行包
sh scripts/build-release.sh
# 静态发行包（需先 rustup target add x86_64-unknown-linux-musl）
sh scripts/build-release.sh --static
```

重新生成文档中的透明 PNG：`python3 scripts/render-doc-images.py`。脚本直接导出 HUD 渲染结果，保留 alpha 通道，不连接桌面或接管输入，仅使用 Python 标准库。

详见 [架构与实机检查](architecture.md)；后续功能候选见 [快捷键设计](shortcuts-roadmap.md)（其中 M、?、1/2/3 已实现，其余仍为候选）。Apache-2.0，项目与第三方说明见 [NOTICE](../NOTICE)。
