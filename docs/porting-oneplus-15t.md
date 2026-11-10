# 把 Skyward 移植到 OnePlus 15T（`fairlady`）

参考设备树：[koaaN/android_device_oneplus_fairlady-orangefox](https://github.com/koaaN/android_device_oneplus_fairlady-orangefox)（一个已知能开机的 OrangeFox recovery）

本文把「需要做哪些工作」拆成可验收的阶段。标注约定：**[事实]** 可在代码或设备树里核对；**[待验证]** 必须用实机/镜像确认后才算数。

---

## 1. 结论

**能移植，而且比想象中省事：Skyward 的架构和这台机器的形态是对得上的。**
两者的共同点不是"都是安卓"，而是同一套具体机制：

| Skyward 的硬前置 | OnePlus 15T 的情况 | 判断 |
| --- | --- | --- |
| 有独立 `recovery` 分区，整分区镜像、末尾带 AVB footer | 有 `recovery_a/b`（`BOARD_RECOVERYIMAGE_PARTITION_SIZE = 104857600`，`FOX_AB_DEVICE_WITH_RECOVERY_PARTITION=1`）**[事实]** | ✅ 对齐 |
| boot header v3/v4（ramdisk-only 布局） | `BOARD_BOOT_HEADER_VERSION := 4` **[事实]** | ✅ 对齐 |
| ramdisk 用 LZ4 打包的 newc | `BOARD_RAMDISK_USE_LZ4`、`OF_USE_LZ4_COMPRESSION` **[事实]** | ✅ 对齐 |
| ramdisk 里有 `system/bin/recovery` 与 `prop.default` | AOSP 血统的 ColorOS recovery（`init.recovery.qcom.rc`、`recovery.fstab` 都是 AOSP/QTI 模板）**[事实]**，具体条目 **[待验证]** | ⚠️ 取证 |
| A/B + super（liblp）+ virtual A/B | `AB_OTA_UPDATER := true`、`qti_dynamic_partitions`、`FOX_VIRTUAL_AB_DEVICE=1` **[事实]** | ✅ 对齐 |
| QTI 解密栈（qseecomd / KeyMint / Gatekeeper / Weaver） | 同一代 QTI（SM8850）**[事实]** | ✅ 同类，服务名要替换 |

**真正的工作量不在 `devices/*.toml`，而在三处：**

1. **拿到与当前系统同一版本的原厂 `recovery.img`**（Skyward 以它为底，替换掉 `system/bin/recovery`）。这决定了后面一切，也是唯一可能需要"发明流程"的地方（见 §3）。
2. **解密**：ColorOS 16 用 KeyMint **v4**（Rust 实现 `onekeymint-service-qti`）+ Gatekeeper + NXP Weaver/StrongBox + 两张 OPLUS 证明 TA。Skyward 的 crypto 层已经实现了 AIDL KeyMint / AIDL+HIDL Gatekeeper / AIDL Weaver / sharedsecret / `wrappedkey_v0`，**不必搬 OrangeFox 那套 blob**，只要把设备树里那份 HAL 清单翻成 `[[service]]` 配置；难点是"能不能在 recovery domain 里把这些 HAL 跑起来"（SELinux、VINTF、servicemanager），不是算法。
3. **触摸**：这台机器的触摸是 **OPLUS 用户态算法栈**（`vendor-oplus-hardware-touch-V2-hbp5-service` + `/odm/firmware/tp/fairlady/` + `oplus_bsp_*` 内核模块），不是"内核里直接出点"。Skyward 现在假设触摸由内核 evdev 直接给，所以需要把**厂商触摸服务**当 `[[service]]` 拉起来；而 `[[service]]` 目前缺 `socket`/`env` 两种 init 语义（见 §5-C）。

**建议的推进顺序**：取证（不改码）→ 造出第一版镜像能进 Skyward（显示 + ADB）→ 触摸 → 解密 → 备份/刷写 → WLAN/振动/更新 → 双启动。**双启动最后做**：它依赖 bootloader 是否理会 `misc` 里的 `boot-recovery` 请求，这是本项目唯一一处依赖"厂商 bootloader 行为"的地方。

---

## 2. 两个项目的职责边界（先想清楚，少走弯路）

OrangeFox/TWRP 的做法：把**解密/触摸/振动需要的二进制、库、固件都塞进 recovery ramdisk**（该设备树 `recovery/root/` 约 27 MB）。
Skyward 的做法：只换掉 `system/bin/recovery`，**把"手机自己的程序"从已安装系统的分区里跑起来**——开机时把 super 里的 `system/vendor/odm/...` 映射并挂载（`system → /mnt/system`，`vendor`/`odm` 等按根目录是否已有同名目录，挂到 `/vendor`、`/odm` 或 `/mnt/<name>`），再按配置拉起 HAL（见 `crates/skyward/src/main.rs` 的 `bring_up`）。

因此：

- **不要把 OrangeFox 的 blob 整包搬进 Skyward 的 ramdisk。** 你要的是它当"设备事实字典"：哪些程序、什么路径、什么就绪信号、哪些 sysfs/固件/权限。
- **好处**：HAL 版本天然和已装系统匹配（这正是 KeyMint/keystore 版本敏感的解密所需要的），ramdisk 也小得多。
- **代价**：Skyward 必须能把已装系统的程序跑起来——这一步在小米机型上已被验证，但在 ColorOS 的 SELinux/VINTF 上要实测（§7）。

---

## 3. P0 取证（不改一行代码，1–2 天）

目标：把"是否成立"从推断变成事实。按顺序做，做完再决定改什么。

### 3.1 备份并取得原厂 `recovery.img`

优先级从上到下：

1. **从机器另一个槽 dump（最稳）**。`fastboot flash recovery` 只写当前槽，另一个槽通常还是原厂 recovery：
   ```sh
   # 在 OrangeFox 的终端里（root shell）
   getprop ro.boot.slot_suffix                      # 例如 _b
   ls -l /dev/block/by-name/ | grep -E 'recovery|boot|init_boot|vendor_boot|dtbo|vbmeta'
   dd if=/dev/block/by-name/recovery_a of=/sdcard/stock-recovery-a.img bs=1M   # 换成"另一个"槽
   dd if=/dev/block/by-name/boot_a of=/sdcard/stock-boot-a.img bs=1M           # 顺带留作回滚
   dd if=/dev/block/by-name/vendor_boot_a of=/sdcard/stock-vendor_boot-a.img bs=1M
   dd if=/dev/block/by-name/init_boot_a of=/sdcard/stock-init_boot-a.img bs=1M
   dd if=/dev/block/by-name/dtbo_a of=/sdcard/stock-dtbo-a.img bs=1M
   dd if=/dev/block/by-name/vbmeta_a of=/sdcard/stock-vbmeta-a.img bs=1M
   ```
   然后 `adb pull` 出来。**这是"整分区 + 末尾 AVB footer"的形态，正好满足 `skyward-pack`**。

2. **ColorOS/OxygenOS 全量包**（版本号必须和当前系统一致）：包里是 `payload.bin`，Skyward 的 `skyward-pack stock` 用的 `unpayload` 天然支持；但要看 payload 里**到底有没有名为 `recovery` 的分区**。不想下 8 GB 的话，用任何支持 HTTP Range 的在线/本地 payload 工具先"列清单、再取单个分区"即可（社区工具 `unota`、`payload-extractor`、紫罗兰工具箱都是这个原理，和 `unpayload` 同类）：
   ```sh
   # 目标：在不下整包的情况下，确认 payload 的分区清单里是否有 recovery
   # 有 → [stock] 原样可用；没有 → 见 §5-B
   ```
   ⚠️ **OnePlus/OPPO 从 Android 16 起改用"动态下载链接"**（生成后约 10 分钟过期），所以 `[stock].package` 很难填成长期有效的直链；可行做法是把同一份固件镜像到自己的对象存储/GitHub Release，或干脆不用 `[stock]`、用本地镜像直接 `--stock`（见 §5-B / §7-10）。

3. **官方 OFP/固件包**：老格式里有独立 `.img`；新格式多半也是 payload 容器。

> 版本一致性：Skyward 在运行时会把 `ro.build.version.release` / `ro.build.version.security_patch` / `ro.vendor.build.security_patch` 换成**已安装系统**的值（等价于 OrangeFox 的 `prepdecrypt.sh SETPATCH`），所以底包不要求是"刚刚好"的版本；但**已安装系统的 vendor HAL 必须和已安装系统一致**（这本来就是同一份），因此 dump 哪个补丁级别的 recovery 都可以用。

### 3.2 解剖镜像，确认 Skyward 的四个硬前置

```sh
cargo run --release -p skyward-pack -- list stock-recovery-a.img | head -40
# 或
target/release/skyward-pack list stock-recovery-a.img | grep -E 'header|system/bin/recovery|prop.default|^.*vendor/bin'
```
判定：

- `header v4`、`kernel 0 bytes`（或无 kernel）→ 与小米同构，走原路。
- ramdisk 能解出来（LZ4）→ ①通过。
- ramdisk 里存在 `system/bin/recovery`、`prop.default` → ④通过。缺任何一个，都要在 packer 里补（§5-B）。
- 顺便看一眼 `system/bin/`、`vendor/lib64/` 里有什么，判断 stock recovery 自带多少东西（不影响设计，但影响调试预期）。

同时拿 OrangeFox 成品 img 对照（可选）：确认它是 ramdisk-only（无 kernel）还是带 kernel——这决定"bootloader 从哪个分区取内核"，也决定你要不要 `skyward-pack boot --kernel` 那条路。

### 3.3 记录设备事实（后面填配置要用）

在 **ColorOS 系统**里（不是 recovery）执行，把输出存下来：

```sh
getprop | grep -iE 'oplus|coloros|version|marketname|sf.lcd|boot.hardware|slot' | sort
wm size; wm density
ls /vendor/bin/hw | grep -iE 'keymint|gatekeeper|weaver|secure_element|boot|health|touch|vibrator'
ls /vendor/lib/modules /vendor_dlkm/lib/modules 2>/dev/null | grep -iE 'tp|touch|wlan|cnss|qca|icnss|haptics|awinic'
ls /odm/firmware/tp/ 2>/dev/null; ls /odm/etc/vibrator 2>/dev/null
getprop ro.build.version.ota; getprop ro.build.display.id
```
以及在 **OrangeFox 里**（能拿到只读挂载后的分区）：
```sh
ls /odm/bin/hw; ls /odm/lib64 | head; cat /proc/cmdline
ls /sys/class/backlight; cat /sys/class/backlight/*/max_brightness
cat /sys/module/msm_drm/parameters/dsi_display0        # 面板名
ls /proc/device-tree/soc | head                        # 面板节点在不在
dmesg | grep -iE 'avc|touch|hbp|wlan|keymint'          # SELinux 是否有拒绝
```

**这些输出决定了配置里 80% 的字段值**，尤其是：`os_version_prop`、`density`、触摸服务名/路径/依赖、WLAN 驱动名。

---

## 4. 配置草案 `devices/fairlady.toml`

> ⚠️ **先不要提交这个文件。** CI 的 `devices` job 会枚举 `devices/*.toml` 并为每个设备构建，`[stock]` 缺失/地址无效会让 CI 变红。等 §3 取证完成、能出镜像了再落地。

```toml
[device]
codename = "fairlady"
brand = "OnePlus"
model = "OnePlus 15T"
ab = true
soc = { hans = "第五代骁龙®8至尊版移动平台", hant = "Snapdragon® 8 Elite Gen 5 行動平台", en = "Snapdragon® 8 Elite Gen 5 Mobile Platform" }
# TODO(待验证)：ColorOS 的版本属性。候选 ro.build.version.ota / ro.build.display.id / ro.oplus.version
os_version_prop = "ro.build.version.ota"

[display]
width = 1216          # 6.32" 1216×2640
height = 2640
density = 480         # TODO：ro.sf.lcd_density；为 0 会让 UI 直接报错退出
diagonal = 6.32
corner_radius = 0     # TODO：系统 overlay 的 rounded_corner_radius

[modules]
sources = ["vendor_boot", "vendor_dlkm", "vendor", "system_dlkm"]
# OnePlus 15T 的触摸：Synaptics S3910 + 天马 AA616，走 OPLUS hbp5 用户态算法；
# 具体清单以 §3.3 抓到的 /vendor_dlkm/lib/modules 为准（下面是从 OrangeFox 树筛出的触摸相关项，
# 里面混着多颗 IC 的驱动，只保留本机真实存在的那些——列了不存在的模块会让加载失败）
load = [
  "oplus_bsp_tp_common", "oplus_bsp_tp_notify", "oplus_bsp_tp_custom",
  "oplus_bsp_tp_syna_common", "oplus_bsp_tp_tcm_S3910", "oplus_bsp_synaptics_tcm2",
]

# OPLUS 的两张证明 TA（"OPLUS SM8850 Attestation"）在 modem 分区的 firmware_mnt/image 里
[firmware]
partition = "modem"

[partitions]
mounted = ["persist"]

# ── 解密链：路径都在"已安装系统"的分区上，由 Skyward 挂载后执行 ──
[[service]]
path = "/vendor/bin/qseecomd"
libraries = ["/system/lib64", "/vendor/lib64"]
ready_property = "vendor.sys.listeners.registered"
user = "root"
groups = ["root", "drmrpc"]

[[service]]
path = "/vendor/bin/hw/android.hardware.security.onekeymint-service-qti"
libraries = ["/system/lib64", "/vendor/lib64"]
ready_service = "android.hardware.security.keymint.IKeyMintDevice/default"
registers = [
  "android.hardware.security.keymint.IRemotelyProvisionedComponent/default",
  "android.hardware.security.secureclock.ISecureClock/default",
  "android.hardware.security.sharedsecret.ISharedSecret/default",
]
user = "system"
groups = ["system", "drmrpc"]

[[service]]
path = "/vendor/bin/hw/android.hardware.gatekeeper-service-qti"
libraries = ["/system/lib64", "/vendor/lib64"]
ready_service = "android.hardware.gatekeeper.IGatekeeper/default"
user = "system"
groups = ["system"]

# NXP eSE 里的 weaver（OPLUS 定制），在 odm 上
[[service]]
path = "/odm/bin/hw/android.hardware.weaver-service.nxp"
libraries = ["/system/lib64", "/vendor/lib64", "/odm/lib64"]
ready_service = "android.hardware.weaver.IWeaver/default"
user = "root"
groups = ["root", "drmrpc"]
capabilities = ["WAKE_ALARM"]

[[service]]
path = "/vendor/bin/hw/android.hardware.boot-service.qti"
libraries = ["/system/lib64", "/vendor/lib64"]
ready_service = "android.hardware.boot.IBootControl/default"
user = "root"
groups = ["root"]

# ── 触摸：OPLUS 用户态算法服务（需要 §5-C 的 socket 支持）──
[[service]]
path = "/odm/bin/hw/vendor-oplus-hardware-touch-V2-hbp5-service"
libraries = ["/odm/lib64", "/vendor/lib64", "/system/lib64"]
ready_service = "vendor.oplus.hardware.touch.IOplusTouch/default"
capabilities = ["SYS_NICE"]
user = "root"
groups = ["root", "system"]

# 触摸/算法服务要打开的节点与 /proc 文件（从 OrangeFox 的 vendor-oplus-...-service.rc 抄）
[[owners]]
user = "system"
group = "system"
mode = 0o664
paths = ["/dev/hbp_core", "/dev/hbp0", "/dev/hbp1", "/dev/tp_misc",
         "/proc/touchpanel/oplus_tp_direction", "/proc/touchpanel/framework_mode"]

[backup]
efs = ["modemst1", "modemst2", "fsg", "fsc", "persist"]
data_skipped = []      # TODO：ColorOS 的安全世界目录（vendor/tzstorage、vendor/qwes、指纹…）
[wipe]
logs = []              # TODO：ColorOS 的日志目录（/data/oplus/… 之类）

# 取证完再填（也可以完全不填，直接给 skyward-pack recovery 一个本地 --stock）
# [stock]
# version = "…"
# package = "https://…/…zip"
# recovery = "…（该分区镜像的 sha256）"
```

`[update]`（小米 OTA 查询协议）**先别写**：OnePlus 是完全不同的接口，写了也查不到；等 §5-F。
`[haptics]` 先别写：见 §5-E。

---

## 5. 代码改动清单

按"没有它就不行"排序。S ≈ 半天，M ≈ 1–2 天，L ≈ 3 天以上。

### A. 【已改】背光正常上限的回退 — S

2026-11-10 已实现（见 §10.6）：找不到面板的 `mi,*` 属性时回退到 `max_brightness`，不再让 UI 起不来。

`crates/skyward/src/ui.rs` 的 `normal_max()` 只认 `/sys/module/msm_drm/parameters/dsi_display0` + 设备树里的 `mi,*` 属性（`mi,normal-max-brightness-clone` / `mi,max-brightness-normal` / `mi,mdss-dsi-fac-bl-max-level`）。OnePlus 的面板不会有 `mi,*`（多半是 `qcom,*`/`oplus,*`，或根本没这些属性），**三个都没有就 `anyhow` 报错**，而 `Backlight::open()?` 在 `ui::run()` 里是 `?` → **整个 UI 起不来**。

改法：找不到时回退到 `max_brightness`（把 4094 当满量程即可，视觉上只是"最亮档不额外拉高亮度"），并让 `Backlight::open` 在极端情况下也能降级而不是返回错误。顺带把属性名表做成"先 `mi,*` 再 `qcom,*`/`oplus,*`"的候选链。

### B. 【视取证】原厂镜像来源与分区名 — S

- `skyward-pack stock` 里分区名字面量是 `"recovery"`（`crates/skyward-pack/src/main.rs`），`avb::footed` 里的 `partition_name` 也是写死的 `"recovery"`。
- 若 §3.1 证明 payload 里有 `recovery`：**什么都不用改**。
- 若没有：给 `[stock]` 加一个可选 `partition`（默认 `"recovery"`），或允许 `package` 是本地路径，并把 footer 的分区名参数化。
- 另外确认 `prop.default` 与 `system/bin/recovery` 是否都在；缺 `prop.default` 的话由 packer 生成一份（追加那几行 `ro.secure=0` 等）。

### C. 【需改】`[[service]]` 缺 init 的 `socket` 与 `setenv` — S

OPLUS 触摸服务的 rc 里有：
```
socket hidl2touchdaemon_client dgram 0660 root inet
socket hidl2touchdaemon_server dgram 0660 root inet
```
振动服务的 rc 里有：
```
setenv RICHTAP_DEVICE_PATH /dev/awinic_haptic
setenv ENV_RICHTAP_CONFIG_PATH /odm/etc/aac_richtap.config
```
Skyward 的 `[[service]]` 只有 path/args/libraries/oneshot/ready_*/registers/description/when/user/groups/capabilities，**没有 socket 也没有 env**。`android_get_control_socket()` 拿不到 init 建的 dgram socket 时，服务大概率起不来或起而不可用。
改法：给 `Service` 加 `sockets = [{ name, type, mode, user, group }]` 与 `env = {…}`，在 `crates/skyward/src/services.rs` 里于 `exec` 前创建/绑定 socket 并传 fd、设置环境变量（这两件事 init 就是这么做的）。

### D. 【不需要了（取证后）】可配置的属性注入 — S

2026-11-10 取证：stock recovery 的 `prop.default` 里**已经有** `vendor.gatekeeper.is_security_level_spu=0` 与 `vendor.gatekeeper.disable_spu=true`，packer 只是追加 `ro.secure=0` 等、删掉三行 BUILD_VERSION，其余原样保留。所以这一步可以不做。（`[properties]` 仍值得作为通用能力，但不再是移植的前置条件。）
改法：配置加 `[properties]`（`name = value` 列表），在 `bring_up` 早期（`adopt_installed_version` 前后）`setprop`。比"往 ramdisk 里塞 prop.default 行"更干净。

### E. 【可能改 / 可以先不做】振动 — M

Skyward 的振动是走内核 input FF（`qcom-hv-haptics` 波形或按谐振频率合成采样）；OnePlus 15T 是 **Awinic + AAC richtap**，经 `vendor.oplus.hardware.vibrator-service`（AIDL）驱动，还需要 `/odm/etc/vibrator/816/**` 波形与 `/odm/etc/aac_richtap.config`。
两条路：
- 不做（`[haptics]` 留空）：recovery 里没有点击反馈，其他功能不受影响。**推荐第一版这样做。**
- 做：写一个 AIDL vibrator 客户端（`skyward-binder` 已有基础），或先按 §5-C 把服务用 `env`+`socket` 跑起来、再通过 binder 调它。M 级。

### F. 【可能改】在线更新页 — S（关）/ M（接）

`crates/skyward-net/src/release.rs` 是 MIUI 协议（固定 AES key、`v=MIUI-…`、`ro.product.mod_device` 推区域）。OnePlus 完全不同：不写 `[update]` 时页面应当不可用（确认 UI 对 `None` 的处理），否则会出现"检查更新一直失败"。
真要接，就需要摸 OPPO 的 OTA 查询接口 —— 建议放到最后，甚至不做（Skyward 自身更新走 GitHub Releases，与厂商无关）。

### G. 不必改（已通用，别浪费时间）

- `skyward-pack` 的 payload 提取（`unpayload` 支持 `payload.bin`）、AVB footer 生成、ramdisk LZ4 解包/重打包（需构建机有 `lz4`）。
- `skyward-vab`（liblp、slot、snapshot）、`skyward-install`（A/B `payload.bin` OTA → 用 stock ramdisk 里的 `update_engine_sideload`，ColorOS 的 OTA 验签也随之复用）。
- `skyward-crypto`：AIDL KeyMint、AIDL/HIDL Gatekeeper、AIDL Weaver、sharedsecret、`wrappedkey_v0` 都已有实现；ColorOS 那边需要的是"服务拉得起来 + 属性对"。
- `backup` 的 BOOT 列表（含 `recovery`/`vbmeta_vendor`）、EFS 分区名（`modemst1/modemst2/fsc/fsg/persist` 与 twrp.flags 一致）。
- `avbctl` / `resetprop` / `7za` / `zstd` / `unzip`、adb/MTP、DRM 显示、`fstab.qcom` 读取（OnePlus 也叫这个名）。
- `[device] ab` 字段目前全仓库没人读，不用管。

### 调试提示：`bring_up` 是"全有或全无"的

`crates/skyward/src/main.rs` 的 `bring_up()` 里以下任一步失败都会让**整段中断**（后面的 `services::start` 不会执行）：

- `[firmware].partition` 挂不上；
- `[partitions].mounted` 里某个分区挂不上；
- `[modules].load` 里某个模块找不到；
- `[[owners]]` 里某个路径 `chown`/`chmod` 失败（**包括路径根本不存在**）。

现象很有迷惑性：屏幕正常、adb 正常，但触摸/解密"全灭"。所以第一版配置要"宁缺毋滥"，先把节点确认存在；调试时先看
`dmesg | grep -E 'devices not brought up|partitions not mounted|loaded |handed'`。
（若希望更宽容，也可以把这个流程改成"逐项失败只记日志"，属于顺手的小改进。）

### H. 工程/文档（收尾）

- `README.md` / `README.zh-CN.md`：支持列表加一行；刷机说明仍然适用（`fastboot flash recovery`）。
- `.github/workflows/build.yml`：等 `[stock]` 有稳定直链后再让 `fairlady` 进矩阵。
- `crates/skyward-config` 里的机型清单测试、`skyward-preview` 的样例数据（可选）。
- 字体：MiSans 是小米的字体，分发在 OnePlus 的包里要考虑许可；换一套 OFL 字体（或用系统自带）更稳妥。

---

## 6. 分阶段计划与验收

| 阶段 | 做什么 | 验收标准 | 主要涉及 |
| --- | --- | --- | --- |
| **P0 取证** | §3 全部；备份双槽 boot/recovery/vbmeta；记录 props/模块/服务/面板 | 得到 `stock-recovery.img`，`skyward-pack list` 四个前置全过；产出一张"事实表" | 不改码 |
| **P1 能启动** | 写 §4 配置（先只填 device/display/modules/firmware/partitions）；改 A；`skyward-pack recovery` 出镜像，`fastboot flash recovery` + `fastboot reboot recovery` | 屏幕出 Skyward 首页、背光可调、adb shell 可用（`ro.secure=0`）；**先不碰解密** | A、B、配置 |
| **P2 触摸** | §3.3 抓模块清单；改 C；加触摸 `[[service]]` + `[[owners]]` + `[modules].load`；对照 OrangeFox 的 rc 逐条核对 | 手指能点、滑动、能进各页面；`dmesg` 无持续 avc 拒绝 | C、配置 |
| **P3 解密** | 按 §3.3 的 HAL 清单填解密链 `[[service]]`；改 D（若需要属性）；观察 `dmesg`/`/tmp/recovery.log` 里 keymint/gatekeeper 的就绪与错误 | 输入锁屏密码后能挂载 `/data` 并浏览文件；重启后仍能解 | 配置、D，必要时补 crypto |
| **P4 实用功能** | 备份/恢复（填 `[backup]`）、擦除（填 `[wipe]`）、刷 A/B OTA zip（用现成的 update_engine 路径）、文件管理、终端 | 备份出 zip/recovery 包、还原成功；能刷一份 ColorOS 全量包 | 配置为主 |
| **P5 周边** | WLAN（`[wlan]` 全套、`probe`）、电量/温度显示、振动（§5-E）、logd（`[logd]`）、更新页（§5-F） | Wi-Fi 能连、能测速页；电量正确 | 配置 + 可能 E/F |
| **P6 双启动 / 自愈** | 先在实机确认 bootloader 是否理会 `misc` 的 `boot-recovery`（`bcb` 请求）与 `--switch_system=` 之类自定义参数；再决定双启动是否可用、`Flash Skyward again` 在 VAB + "recovery 不参与 AB OTA" 下是否仍然必要 | 能按需回到 Skyward；OTA 后 Skyward 仍在 | `skyward`/`skyward-system-init`/`skyward-vab`，风险最高 |

---

## 7. 风险与未知（按"会不会推翻方案"排序）

1. **bootloader 对 recovery 的验证**：Skyward 写的是**未签名**的 vbmeta hash footer。小米解锁机接受；OnePlus 的 ABL 是否也接受 **[待验证]**。失败表现是 `fastboot reboot recovery` 直接回系统/黑屏。备选：`avbctl disable-verification`（Skyward 自带命令）或先 `fastboot --disable-verity`。OrangeFox 能开机说明解锁后大概率不校验，但**它是不是也写了未签名 footer** 值得用 `avbtool info_image` 对比。
2. **SELinux**：Skyward 拉起的 HAL 继承 `recovery` domain（OrangeFox 的每个 rc 都显式 `seclabel u:r:recovery:s0`，等于同一件事）。ColorOS 的 recovery sepolicy 是否允许 `recovery` 访问 `/dev/smcinvoke`、vendor HAL 注册、`/data/vendor/…` **[待验证]**。若拒绝，要么改 sepolicy（L，ramdisk 里的 `sepolicy` 二进制需要重编），要么退回"把 HAL 塞进 ramdisk + 用 OrangeFox 那份 rc/属性"的方案。
3. **KeyMint v4 兼容**：Skyward 的客户端是 AIDL `IKeyMintDevice`（方法表向后兼容，理论上能吃 v4 服务），但 ColorOS 的 HAL 是 Rust + SMCInvoke 实现，可能有额外要求（属性、`vendor.sys.listeners.registered` 时序、OS version/patchlevel 上报）**[待验证]**。失败表现是 keymint 起不来或 `generateKey` 报 `UNKNOWN_ERROR`。
4. **keystore 数据库布局**：Skyward 直接读 `/data/misc/keystore/persistent.sqlite` 并用 KeyMint 解 `keymaster_key_blob`；ColorOS 的 keystore2/KeyMint v4 是否仍是这个 schema **[待验证]**（AOSP 血统下大概率是）。
5. **触摸服务的 init 语义**：socket/env/`/odm` 挂载点/固件路径/`force_sysfs_fallback`（这一条 Skyward 已经自动写 1）。若不满足，服务起来了也不出点。**这是最可能在 P2 卡住的地方。**
6. **VAB 交互**：`FOX_VIRTUAL_AB_DEVICE=1`，OTA 走 snapshot/COW。Skyward 已经会拒绝"snapshot 未合并时写动态分区"，但双启动直接写 `boot`/`init_boot` 与 OTA 快照的关系 **[待验证]**；而且写 `boot` 分区在 ColorOS 上可能触发校验/回滚。
7. **`misc` BCB 语义**：AOSP 格式两边一致（Skyward 用标准 2048 字节布局），但**厂商 bootloader 是否按 `boot-recovery` 进 recovery** 是它自己的事。`--switch_system` 这种自定义参数只有小米的 bootloader 认。
8. **字体许可**：MiSans 随包分发在非小米设备上的许可问题（可换 OFL 字体）。
9. **原厂镜像的可复现性**：OnePlus/OPPO 的动态下载链接会过期，CI 的 `skyward-pack stock`（URL + SHA-256）不能直接照抄小米那套。要么自己镜像固件，要么给 packer 增加"本地镜像/自定义镜像源"的入口（§5-B），并把 `stock/recovery.img` 放进 GitHub Release 缓存。

---

## 8. 从 OrangeFox 树抄什么（对照表）

| OrangeFox 里的位置 | 用途 | 落到 Skyward 的哪里 |
| --- | --- | --- |
| `BoardConfig.mk` 的 `TW_LOAD_VENDOR_MODULES`（33 个 ko） | 触摸/充电/eSE/NFC/音频模块清单 | `[modules].load`（只取触摸链 + 依赖；依赖由 `modules.dep` 自动排序） |
| `recovery/root/vendor/etc/init/*.rc` | 每个 HAL 的路径/账号/组/caps/就绪属性/属性触发条件 | `[[service]]` 的 `path/user/groups/capabilities/ready_*/when` |
| 同上的 `socket`/`setenv` 行 | init 供给服务的 socket 与环境变量 | §5-C 新增的 `sockets`/`env` |
| `recovery/root/vendor/odm/etc/init/vendor-oplus-...-touch...rc` 的 `on boot`/`on post-fs-data` 段 | `/dev/hbp*`、`/proc/touchpanel/*`、`/data/vendor/touch/**` 的属主与权限 | `[[owners]]` |
| `recovery/root/vendor/bin/prepdecrypt.sh` | 从已装系统校正 OS version/patchlevel、判断 fastboot boot | Skyward 已内建等价逻辑（`adopt_installed_version`），不用抄 |
| `recovery/root/vendor/firmware_mnt/image/*.mdt|.b0X` 两张 OPLUS 证明 TA | TEE 加载的证明/StrongBox 应用 | `[firmware] partition = "modem"`（从已装系统的 modem 分区取，不用塞 ramdisk） |
| `recovery/root/vendor/etc/gpfspath_oem_config.xml`、`ssg/ta_config.json` | QSEE/SSG 的路径与 TA 配置 | 不用抄（随已装系统的 vendor 生效，或由服务自读） |
| `recovery/root/vendor/odm/firmware/tp/fairlady/**` | 触摸固件 + 算法库 | 不用抄（在已装系统的 odm 上，服务自己读 `/odm/firmware/tp/`） |
| `recovery/root/vendor/odm/etc/vibrator/**`、`aac_richtap.config` | 振动波形与配置 | 暂不需要（§5-E 不做就跳过） |
| `recovery/root/system/etc/recovery.fstab`、`twrp.flags` | 分区/加密参数/挂载点 | **不用抄**：Skyward 读已装系统的 `fstab.qcom`，只把 `/data`、`/metadata` 的加密参数当运行时事实 |
| `recovery/root/init.recovery.usb.rc`（0x2717/0x4EE7、adbm tp/fastbootd 三组） | USB 三模式 | **不用抄**：Skyward 自己配 configfs gadget |
| `init.recovery.qcom.rc` 的 `on fs`（bootdevice 符号链接、`force_sysfs_fallback`） | 让固件加载与 by-name 路径可用 | 已由 Skyward 内建（`FIRMWARE_FALLBACK` 等） |
| `security/*.x509.pem` | OTA 验签 | 不用抄：更新走 stock ramdisk 里的 `update_engine_sideload`，验签用它自己的 otacerts |
| `system.prop` 的 `vendor.gatekeeper.is_security_level_spu=0` 等 | HAL 行为开关 | `[properties]`（§5-D） |

---

## 9. 一页纸的下一步

1. 备份双槽 `boot/recovery/init_boot/vendor_boot/dtbo/vbmeta`，dump 出原厂 `recovery.img`（§3.1）。
2. `skyward-pack list` 跑一遍，把四个前置的结论记下来。
3. 按 §4 写 `devices/fairlady.toml`（**先不提交**），只填 `[device]`/`[display]`/`[modules]`/`[firmware]`/`[partitions]`。
4. 改 §5-A（背光回退），出第一版镜像，刷入，确认能进首页 + adb。
5. 再按 §3.3 的事实填触摸与解密，逐项验收。


---

## 10. 取证结果（2026-11-10，实机 PLZ110 / ColorOS 17.0.1.200）

全部用 adb 从真机/真包上读出来，不是推断。

### 10.1 设备

| 项 | 值 |
| --- | --- |
| 型号 / 设备码 | `PLZ110` / `OP64DDL1`（与 OrangeFox 树 `TARGET_DEVICE_ALT` 一致） |
| 系统 | ColorOS `17.0.1.200(CN01)`，`ro.build.version.ota=PLZ110_11.C.18_1180_202611040742` |
| Android | **17 / SDK 37**（`ro.build.version.release=17`） |
| 平台 | `canoe` / `sm8850`，`ro.hardware=qcom`，`ro.board.platform=canoe` |
| 槽位 / 动态分区 / 加密 | `_a` / `true` / `file`（FBE），`ro.virtual_ab.enabled=true`（含 userspace snapshots） |
| 屏幕 | 物理 `1216x2640`，`ro.sf.lcd_density=560`，`wm density` 覆盖值 **476**，`vendor.display.lcd_density=480` |
| 面板参数 | `/sys/module/msm_drm/parameters/dsi_display0` 存在（shell 读不到，root 可读） |
| 背光 | `/sys/class/backlight/panel0-backlight`（OrangeFox 树：max 4094 / 默认 2047） |
| 振动 | `qcom_hv_haptics` + `swr_haptics_dlkm` 已加载，`/sys/class/qcom-haptics` 存在 |
| 触摸 | Synaptics **S3910** + 天马 AA616，OPLUS **hbp5** 用户态算法；`/dev/hbp0`、`/dev/hbp_core`（system:system 0660）、`/dev/hbp_report0`（root 0600） |
| 触摸模块链 | `oplus_bsp_tp_tcm_S3910` → `syna_common,tp_common,tp_custom,tp_notify,dft_olc,fw_update,panel_event_notifier`；`oplus_bsp_tp_hbp_syna_s3910` → `hbp_core,tp_notify,dft_olc,panel_event_notifier` |
| 触摸固件/算法 | `/odm/firmware/tp/fairlady/`（`libafe_s3910.so`、`vnd_tp_fw_main.bin`、TFLite 模型等） |
| Wi-Fi | `qca_cld3_peach_v2` + `cnss2` 已加载（与小米 17 Ultra 同平台方案）；`/vendor/bin/hw/wpa_supplicant`，服务名 `android.hardware.wifi.supplicant.ISupplicant/default` |
| 分区 | `recovery_a/b`、`boot_a/b`、`init_boot_a/b`、`vendor_boot_a/b`、`dtbo_a/b`、`vbmeta{,system,vendor}_a/b`、`super`、`metadata`、`userdata`、`persist`、`modem_a/b`、`modemst1/2`、`fsc`、`fsg`、`oplusreserve1/2/3/5`；**没有独立 `cache`**（cache = `oplusreserve2`） |

### 10.2 全量包与抽取到的原厂 recovery

`/sdcard/Download/my_manifest_PLZ110_11.C.18_1180_202611040742.97.1da82baa_all.zip`（9,588,647,825 字节）

- 是标准 zip（ZIP64），内含 `payload.bin`（9,588,641,649 字节）、`payload_properties.txt`、`care_map.pb`、`META-INF/...`。
- `payload.bin` 头是**大端**的（magic `CrAU`、version 2、manifest 351,147 字节、metadata signature 523 字节）。
- manifest 里 **67 个分区**，`recovery` 在列，且带 25 段 `REPLACE_XZ`（其余为 ZERO/DISCARD）。注意 `install_op.type` 的枚举是 `ZERO=6, DISCARD=7, REPLACE_XZ=8`（不是 6=XZ）。
- 这是**全量包**：67 个分区全部带真数据（compressed data 合计 9,588,289,432 字节，≈ payload 大小），不是增量包。
- 已顺带抽出回滚所需的整套镜像（每个都与 manifest 的分区哈希校验一致）：

| 镜像 | 大小 | SHA-256 |
| --- | --- | --- |
| `stock-boot.img` | 100,663,296 | `b35ccccf9ebfc69fd461aec12d66f03ddcb4a56fec5d73be2f44bd7adbb0fbaa` |
| `stock-init_boot.img` | 8,388,608 | `b4d73f033b7aea63c95b3420b03faa6df39871e3ab29958c25a2c6797e35f21d` |
| `stock-vendor_boot.img` | 100,663,296（`VNDRBOOT`） | `90441765b7c5f11c49b36f04c76088ed48034b8b78bdc49efde9ec614e1a9355` |
| `stock-dtbo.img` | 25,165,824 | `6942d7a07f915c503913113d516c39618d7d5853e971d167b70f1058440598f0` |
| `stock-vbmeta.img` | 8,192 | `c8f299d88f338b29a08c718f086d803574a958f20c2ee1b734d0ba2e3dc3b640` |
| `stock-vbmeta_system.img` | 4,096 | `c84d111920d22966aab83c6d99e563c0f52f876aa37c602a47ea4ca33837d80f` |
| `stock-vbmeta_vendor.img` | 4,096 | `e6381c78ac7ecfe6b580e6d626640960d80f53ee7fa54dd1706552502a68d95d` |

（`boot` 镜像末尾没有 `AVBf` footer，校验走 `vbmeta` 的哈希描述符；`recovery` 则有 footer——Skyward 的打包正是照这个形态做的。）

抽出的原厂 recovery：`recovery.img`

| 项 | 值 |
| --- | --- |
| 大小 | 104,857,600（整分区，= BoardConfig 的 100 MiB） |
| header | **v4**，`kernel_size = 0`（ramdisk-only） |
| ramdisk | **LZ4 legacy**（`02 21 4C 18`），49,929,458 字节 → 解包 101,860,352 字节 cpio |
| AVB | 末尾有 `AVBf` footer（`original_image_size = 49,934,336`、`vbmeta_size = 2240`） |
| SHA-256 | `f4826442eeaf2487c42d3ac7cfb34f73be64c806f82bc9dfc42eeb881c9aa3e3`（也是 payload 里的分区哈希，校验通过） |

**结论：Skyward 的四个硬前置全部成立**（`skyward-pack list` 已实测通过：`header v4  kernel 0 bytes  ramdisk 49929458`，`system/bin/recovery` 与 `prop.default` 都在）。

### 10.3 stock recovery ramdisk 里有什么（决定要不要 overlay）

- `system/bin/recovery`（3.2 MB，AOSP 血统）、`system/bin/init`、`system/bin/update_engine_sideload`、`system/bin/fastbootd`
- `prop.default`（28 KB）、`sepolicy`（2.1 MB）、`etc/recovery.fstab`、`init.recovery.qcom.rc`、`ueventd.qcom.rc`
- `system/bin/hw/`：只有 `boot-service.qti.recovery`、`health-service.qti_recovery`、`fastboot-service.example_recovery`
- **没有** `qseecomd`、`keymint`、`gatekeeper`、`weaver`、`keystore2`、`logd`；`vendor/bin/hw` 是空的；`vendor/lib64` 只有 6 个文件
- 结论：**解密/触摸必须用"已安装系统"的 HAL**（Skyward 的既有设计），overlay 方案不需要

### 10.4 两个必须处理的引导事实

1. **入口是自动启动的 `recovery` 服务，不需要改 init.rc**（初判曾以为被 `phoenix_recovery` 挡住，读到 `on boot` 段后修正）：
   `system/etc/init/hw/init.rc` 的 `on boot` 里有 `class_start default`（第 170 行），而 `service recovery /system/bin/recovery` 既没有 `class` 也没有 `disabled`，属默认类，**开机即被 init 拉起**——Skyward 换掉 `system/bin/recovery` 就会被执行。
   同一个默认类里还会起 `phoenix_recovery`（`critical`，OPPO 的引导来源记录/交接小程序：只 import 了 `__system_property_*`、`fopen/rename/chmod`、`socket/connect`，**没有 exec/fork/system**，所以它不会自己启动别的程序）。它是个待观察的并存进程（是否抢屏、是否会因 `critical` 被 init 重启），但不再是"进不了 Skyward"的阻塞项。
   另外 stock init 已经启动了 `servicemanager`、`vendor.boot-qti`（`IBootControl/default`）、`vendor.health-recovery`——配置里**不要重复启动 boot control / health**，否则同名服务会互相顶掉。
   `on property:sys.usb.config=adb` 会 `start adbd`，所以 Skyward 设置 `sys.usb.config=adb` 后 adb 正常；同段里的 `start logd` 会失败（ramdisk 里没有 `logd`），非致命。
2. **版本基座不一致**：stock recovery 是 **Android 16** 构建（`ro.build.version.release=16`、SDK 36、fingerprint `oplus/ossi/ossi:16/BP2A.250605.015`、2026-11-03），而已安装系统是 **Android 17 / SDK 37**。Skyward 会把 `ro.build.version.*` 改成已装系统的值（KeyMint 需要），但它的用户态程序仍是 A16 的；运行 A17 的 vendor HAL 依赖 Skyward 的"借已装系统库"机制，这一点必须实机验证。

### 10.5 已安装系统侧的 HAL 清单（Skyward 要跑的程序）

| 用途 | 路径 | 就绪判据 |
| --- | --- | --- |
| QSEE 通道 | `/vendor/bin/qseecomd` | `ready_property = "vendor.sys.listeners.registered"` |
| KeyMint v4（默认） | `/vendor/bin/hw/android.hardware.security.onekeymint-service-qti` | `android.hardware.security.keymint.IKeyMintDevice/default`（另有 RKP / secureclock / sharedsecret） |
| Gatekeeper（AIDL、Rust） | `/vendor/bin/hw/android.hardware.gatekeeper-rust-service-qti` | `android.hardware.gatekeeper.IGatekeeper/default` |
| Weaver（NXP） | `/odm/bin/hw/android.hardware.weaver-service.nxp` | `android.hardware.weaver.IWeaver/default`（还有 `vendor.oplus.hardware.weaver.IWeaverExt/default`） |
| SecretKeeper | `/vendor/bin/hw/android.hardware.security.secretkeeper-service-qti` | `android.hardware.security.secretkeeper.ISecretkeeper/default`（Skyward 目前没有客户端，见 §10.7） |
| StrongBox | `/odm/bin/hw/android.hardware.security.keymint-service.strongbox-nxp` | `...IKeyMintDevice/strongbox` |
| Boot control | `/vendor/bin/hw/android.hardware.boot-service.qti` | `android.hardware.boot.IBootControl/default` |
| Health | `/vendor/bin/hw/android.hardware.health-service.qti` | `android.hardware.health.IHealth/default` |
| 触摸 | `/odm/bin/hw/vendor-oplus-hardware-touch-V2-hbp5-service` | `vendor.oplus.hardware.touch.IOplusTouch/default`（要 `SYS_NICE` + 两个 dgram socket） |
| Wi-Fi | `/vendor/bin/hw/wpa_supplicant` | `android.hardware.wifi.supplicant.ISupplicant/default` |

`/vendor/etc/fstab.qcom` 里有 `modem → /vendor/firmware_mnt`、`/metadata`、`/data`（`wrappedkey_v0` + `keydirectory=/metadata/vold/metadata_encryption`），`[firmware] partition = "modem"` 因此可以照写；`persist` 已被 stock init 以 rw 挂到 `/mnt/vendor/persist`，Skyward 的 `fstab::mount` 见到已是挂载点会直接复用，不必再写进 `[partitions].mounted`。

### 10.6 第一版镜像（已构建，未刷入）

- 补丁：`crates/skyward/src/ui.rs` 的背光上限新增回退（面板没有 `mi,*` 属性时，整段亮度就是正常上限），`cargo test -p skyward` 通过。
- 配置草案：`/tmp/fairlady/fairlady.toml`（device/display/modules/firmware/backup + 6 个解密服务 + 触摸服务，尚未提交进 `devices/`，因为 CI 会自动构建 `devices/*.toml`）。
- 产物：`/tmp/fairlady/skyward-fairlady.img`，104,857,600 字节（整分区），ramdisk 70,253,904 字节，`system/bin/recovery` = 8,596,656 字节（Skyward），`skyward/{device.toml,system-init,字体}` 就位，末尾 `AVBf` footer；SHA-256 `a252b39fdcab5e63d34344eed79f3c8410e0d0ca67ea7383f2a5128890516ac8`。配置里已去掉 boot control（stock init 已启动它）。

刷入与回滚（需要用户自己执行；`recovery.img` 就是上面校验过的原厂镜像）：

```sh
adb reboot bootloader
fastboot flash recovery /tmp/fairlady/skyward-fairlady.img
fastboot reboot recovery

# 回滚
fastboot flash recovery /tmp/fairlady/recovery.img
```

### 10.7 取证后的改动清单（替换 §5 里对应的项）

| 原条目 | 结论 |
| --- | --- |
| §5-A 背光回退 | **已实现**（必须，否则 UI 起不来） |
| §5-B packer 分区名/来源 | **本地构建不需要**（`recovery` 分区在 payload 里）；只有要做 CI 自动下载时才需要解决"OPPO 动态链接过期" |
| §5-C `[[service]]` 的 socket/env | **仍然需要**（触摸服务要 2 个 dgram socket；振动服务要 `RICHTAP_*` env） |
| §5-D 属性注入 | **不需要了**（stock `prop.default` 已带 `vendor.gatekeeper.*`） |
| §5-E 振动 | 可能**比预想简单**：`qcom-hv-haptics` 在，`[haptics] resonance = "/sys/class/qcom-haptics/lra_frequency_hz"` 待 root 下确认存在 |
| §5-F 更新页 | 仍然：不写 `[update]` 即可 |
| 新增 1 | ~~phoenix_recovery 拦截~~ → **已排除**：`recovery` 服务属默认类，`on boot` 的 `class_start default` 会拉起它（§10.4-1）。只需观察 phoenix 与 Skyward 并存是否抢屏 |
| 新增 2 | **A16 recovery / A17 系统的版本错配**（§10.4-2）——重点验证 vendor HAL 能否在 A16 用户态下跑起来 |
| 新增 3 | **SecretKeeper**：ColorOS 17 注册了 `ISecretkeeper/default`；Skyward 只有 weaver 客户端。用户密钥是否还走 weaver 需实机验证，若走 secretkeeper 就要补一个客户端（M） |
| 新增 4 | **logd 在 stock ramdisk 里不存在**：`[logd]` 别写，写了也只是 5 秒空等（非致命） |


---

## 11. vbmeta 移植（2026-11-10）

### 11.1 为什么需要它：链式 vbmeta 的机制（本机实测）

把 `vbmeta`（根）分区和 `recovery` 分区的 vbmeta 解析出来对照，链是这样的：

| 位置 | 内容 |
| --- | --- |
| 根 `vbmeta` | 签名 `SHA256_RSA4096`，公钥 SHA-256 以 `8d897f62…` 开头；带 **5 个 CHAIN_PARTITION** 描述符：`boot`、`dtbo`、`recovery`（这三条**内嵌同一把 RSA4096 公钥**，即 `8d897f62…`）以及 `vbmeta_system`、`vbmeta_vendor`（各用自己的 RSA2048 公钥） |
| `recovery` 分区 | 末尾 AVB footer 里的 vbmeta：`SHA256_RSA4096`、公钥同上 `8d897f62…`、`rollback_index = 1`、`avbtool 1.3.0`、含 `recovery` 的 HASH 描述符（`image_size = 49,934,336`，即原厂镜像长度） |
| `vbmeta_system` / `vbmeta_vendor` | 各自 `SHA256_RSA2048`、`rollback_index = 1793491200` |

Skyward 的 `skyward-pack` 生成的 footer vbmeta 是 **`algorithm = NONE`、没有公钥、没有签名**（512 字节）。
根 `vbmeta` 对 `recovery` 的链要求"被链分区的 vbmeta 必须由 `8d897f62…` 这把钥匙签名"，于是：

> 刷入 Skyward → recovery 分区的 vbmeta 变成未签名 → **链校验失败 → vbh / bootstate 报错，设备起不来**。

这正是 `vbmetafixer` 要解决的问题：把**官方（原厂签名）的 `recovery.vbmeta` 移植进镜像**，让链重新成立。

### 11.2 工具与做法

`~/Downloads/vbmetafixer_linux.zip` 里是三个 **Linux x86-64** 程序：

| 程序 | 作用 |
| --- | --- |
| `bin/vbmetabackup` | 通过 adb 从手机 `su -c dd` 拉取 `vbmeta`/`vbmeta_system`/`vbmeta_vendor`/`boot`/`init_boot`/`vendor_boot`/`dtbo`/`recovery` 等分区，有 footer 的取 footer 里的 vbmeta、没有的直接取整块，存成 `<分区>.vbmeta`；同时跟随根 vbmeta 的 chain 描述符 |
| `bin/vbmetaport <source.vbmeta> <target.img> <out.img>` | 读目标镜像 footer 的 `original_image_size`，把**官方 vbmeta** 放到 4096 对齐位置，末尾 64 字节写 footer；输出大小 = 目标大小（整分区不变） |
| `vbmetaflasher [-f fastboot] [分区] [镜像]` | backup → 按分区名找 `vbmetas/<base>.vbmeta` → port → `fastboot flash <分区> <临时镜像>` |

本机是 macOS/arm64，跑不了这些 ELF；所以按 `vbmetaport` 的**同一套布局**做了等价实现（`/tmp/fairlady/vbmetaport.py`），并逐字节校验：

```
目标镜像数据(original_image_size) | 0 填充到 4096 | 官方 vbmeta | 0 填充 | 末尾 64 字节 AVBf footer
```

### 11.3 本次移植

源 vbmeta（等价于 `vbmetabackup` 的 `vbmetas/recovery.vbmeta`，从 OTA 里那份原厂 recovery 的 footer 取出）：

```
/tmp/fairlady/vbmetas/recovery.vbmeta   2,240 字节
SHA-256   4ee594390247d56c430fb04ce50f1b5f56c4ebba74c04de2eace3ae8579d33aa
算法      SHA256_RSA4096   公钥 1032 字节（sha256 8d897f62…）  rollback_index 1
描述符    HASH(recovery, image_size=49,934,336) + PROPERTY(recovery 指纹)
```

产物：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节（整分区）
SHA-256   8c3e69ab750b58edcdc7cff8a56b2b32536940c9426fb173e36a7b23a5626e59
footer    AVBf 1.0  original=70,258,688  vbmeta_offset=0x4301000(70,258,688)  vbmeta_size=2,240
数据区    与未移植版本逐字节相同（Skyward 的 header+ramdisk 没有被改动）
vbmeta    与官方字节完全相同（签名/公钥/rollback 全部保留）
```

对照一下移植前后：

| | 移植前 `skyward-fairlady.img` | 移植后 `skyward-fairlady-vbmeta.img` |
| --- | --- | --- |
| footer vbmeta | 512 B，`algorithm=NONE`，无签名 | **2,240 B，`SHA256_RSA4096`，官方签名** |
| 公钥 | 无 | `8d897f62…`（根 vbmeta 链描述符里那把） |
| rollback_index | 0 | 1（= 原厂值） |
| 数据区 | Skyward header+ramdisk | 不变 |
| 整分区大小 | 104,857,600 | 104,857,600 |

刷入：

```sh
adb reboot bootloader
fastboot flash recovery /tmp/fairlady/skyward-fairlady-vbmeta.img
fastboot reboot recovery
```

### 11.4 注意事项

1. **只适用于已解锁 bootloader**。官方 vbmeta 的 HASH 描述符仍指向原厂镜像内容（`image_size=49,934,336`），与我们的镜像不一致；解锁状态下 bootloader 跳过内容哈希校验、只验链与签名，这正是本工具成立的前提。**刷完不要回锁**。
2. 若设备仍报错，可再补一步：`fastboot flash vbmeta /tmp/fairlady/stock-vbmeta.img`（原厂根 vbmeta，8,192 字节，SHA-256 `c8f299d8…`），把整条链恢复成原厂状态；`vbmeta_system` / `vbmeta_vendor` 同理（`c84d1119…`、`e6381c78…`）。
3. 若以后要在 CI 里自动化，等价流程是：`skyward-pack recovery` → `vbmetaport <官方 recovery.vbmeta> <镜像>`；官方 `recovery.vbmeta` 固定成构建产物（本机这份来自同版本全量包，可复用）。


---

## 12. 第一版镜像的实机结果与修复（2026-11-10 第二轮）

第一版 `skyward-fairlady-vbmeta.img`（§10.6 / §11）在真机上的结果：**能进 UI、能读到屏，但 adb 找不到设备、触摸没有**。

### 12.1 原因分析

1. **触摸服务从不启动（最主要）**：`services::start` 原来是"要么全起、要么全停"——
   - 任何一个 `[[service]]` 的可执行文件不存在，直接 `bail!`，后面的都不启动；
   - 任何一个服务在 20 秒内没有给出就绪信号（属性/服务名），直接 `bail!`，后面的都不启动。
   我们的触摸服务排在解密链后面，而解密链在 ColorOS 的 policy 下大概率起不来（见第 3 点），于是**触摸服务根本没被启动**。
2. **触摸服务缺少 init 给它建的 socket**：`/odm/etc/init/vendor-oplus-hardware-touch-V2-hbp5-service.rc` 里有
   `socket hidl2touchdaemon_client dgram 0660 root inet`、`socket hidl2touchdaemon_server dgram 0660 root inet`，
   服务会去 `/dev/socket/` 打开它们；Skyward 的 `[[service]]` 原来没有 `socket`/`setenv` 语义。
3. **SELinux**：`androidboot.selinux` 未在 vendor_boot 的 bootconfig 里出现（本机取证实测），recovery 是 enforcing；
   而 ColorOS 的 recovery 策略是为它自己的 recovery 程序写的，**没有理由允许执行 `/vendor/bin`、`/odm/bin` 的厂商程序，
   也没有为 adbd 的 functionfs 打开方便之门**（OFox/TWRP 之所以没这个问题，是它整包自带 ramdisk + 策略）。
   这也合理解释了 adb 为什么"没有设备"。

### 12.2 这一轮改了什么

| 改动 | 位置 | 作用 |
| --- | --- | --- |
| `[[service]]` 新增 `sockets` | `crates/skyward-config`、`crates/skyward/src/services.rs` | 按 init 的语义在 `/dev/socket/` 建 stream/dgram/seqpacket socket，按配置设属主与权限，以 `ANDROID_SOCKET_<name>` 环境变量把 fd 交给程序 |
| `[[service]]` 新增 `env` | 同上 | 对应 init 的 `setenv`（振动服务以后要用） |
| `services::start` 不再"全有或全无" | `crates/skyward/src/services.rs` | 缺程序、缺账号、没就绪都只写 kmsg 并继续下一个，触摸不会再被解密链连累 |
| `[device] permissive`（可选） | `crates/skyward-config`、`crates/skyward/src/main.rs` | 写 `/sys/fs/selinux/enforce = 0`；成功后再 `usb::switch("adb")` 把 adbd/USB 重新拉一遍（之前被策略拒绝的，这次不再拒绝）。失败只记日志，不影响其它功能 |
| 配置 | `/tmp/fairlady/fairlady.toml` | 触摸服务挪到第一位、补 `sockets`、打开 `permissive` |

### 12.3 新镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   8f845c6e0cc5e370cb2806ed8d91ffbdd9d920da177c10feb80c84078cc1d953
footer    AVBf 1.0, original=70,279,168, vbmeta@0x4306000, size=2,240（官方 recovery.vbmeta 原样）
```

刷入：

```sh
adb reboot bootloader
fastboot flash recovery /tmp/fairlady/skyward-fairlady-vbmeta.img
fastboot reboot recovery
```

### 12.4 这次要观察的

1. 触摸是否恢复：`/dev/hbp_core` 是否被打开、屏幕能否点按。
2. adb 是否恢复（permissive 之后 `usb::switch("adb")` 会重启 adbd）。
3. 若触摸仍无、adb 仍无，下一轮就把 **sepolicy 层面**的东西补上（给 recovery domain 加执行 vendor 程序/访问 hbp 设备/functionfs 的规则），
   或把触摸/解密所依赖的少数二进制换成 OFox 那种"自带进 ramdisk"的方案作为兜底。


---

## 13. 无 adb 下的自检：开机报告页（2026-11-10 第三轮）

第二轮（§12）的镜像在真机上**触摸和 adb 仍然都不行**。运行时写 `/sys/fs/selinux/enforce` 很可能是被 `recovery` 域的策略拒绝（否则 permissive 之后 adb 应当恢复），但此时既没有 adb 也没有触摸，**拿不到任何日志**。

为此加了一个"不依赖 adb 的日志通道"：开机时把自检报告写成文本，并**作为第一个页面直接显示**（编辑器页，可用音量键/电源键翻看，或直接拍照）。

### 13.1 报告里有什么

```
skyward <版本> — boot report
device: fairlady (OnePlus 15T)
selinux: enforce=0/1
mounted: vendor=? odm=? vendor_dlkm=? system_dlkm=? metadata=?
touch: service=? hbp_core=? hbp0=? sockets=?
services: touch=? qseecomd=? keymint=?（pid 或 no）
usb: config=? state=? adbd=? udc=<configured/not attached/…>
modules: <已加载的 hbp/tp 模块，或 none of touch>
usb gadget: ffs_mounted=? configfs=? usb_f_fs=? libcomposite=?
-- last steps that did not go --
<recovery 日志里含 not brought up / missing / failed / denied 的最后 8 行>
-- kernel log --
<内核日志里含 avc: / skyward: / module / usb / hbp / touch / keymint / qseecom 的最后 12 行>
```

同时写入 `/skyward/run/boot.log`（recovery 里的临时目录；adb 恢复后可直接 `adb pull`）。

### 13.2 这一轮新增的代码

| 项 | 位置 |
| --- | --- |
| `[device] diagnose` 配置 | `crates/skyward-config/src/lib.rs` |
| `skyward_sys::klog()`：读整个内核环（`SYSLOG_ACTION_READ_ALL`） | `crates/skyward-sys/src/lib.rs` |
| `boot_report()`：收集 SELinux/挂载/节点/服务/USB/模块/日志，写 `/skyward/run/boot.log` | `crates/skyward/src/main.rs` |
| 诊断模式下把报告作为首页打开 | `crates/skyward/src/ui.rs` |

### 13.3 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   0dfcd35b6830942d25d1a2576ed9347299c4d544d983b21abfc4843dbd57ef2c
配置      devices/fairlady.toml（草案）：permissive=true、diagnose=true
```

刷入后**不用做任何操作**，直接把屏幕上报告页的内容告诉我（拍照最省事；没有触摸也能看，读完长按电源键关机重开即可）。

拿到报告后按这几条分流：

| 报告里看到 | 说明 | 下一步 |
| --- | --- | --- |
| `selinux: enforce=1` | 运行时的 permissive 没生效 | 改内核命令行或改 recovery 的二进制策略（见下） |
| `mounted: vendor=NO odm=NO` | super 没挂上，服务根本没条件启动 | 修 liblp/挂载（`skyward-vab` 日志） |
| `modules: none of touch` | 触摸模块没加载 | 修 `[modules].sources`/`load` 或看 avc 里的 `module_load` |
| `touch: service=NO` | `/odm` 没挂上 | 同 super 问题 |
| `services: touch=no`（文件在、进程没起） | 执行被拒（avc 里应有 `execute_no_trans`） | 策略补丁，或把二进制搬进 ramdisk |
| `usb: adbd=restarting` / `udc=not attached` | adbd 起来了但 functionfs/UDC 没配好 | 看 avc 里的 `functionfs`/`usb_device` 拒绝 |


---

## 14. 自检报告定位到的真问题（2026-11-10 第四轮）

§13 的诊断镜像在真机上给出的报告（摘录）：

```
selinux: enforce=0                                   ← permissive 生效
mounted: vendor=true odm=true vendor_dlkm=true system_dlkm=true metadata=true
touch: service=yes hbp_core=yes hbp0=yes sockets=yes
services: touch=no qseecomd=pid 603 keymint=pid 646
usb: config=adb state=adb adbd=running udc=configured
modules: … oplus_hbp_core, oplus_bsp_tp_hbp_syna_s3910, panel_event_notifier, oplus_bsp_tp_tcm_S3910 …
-- last steps that did not go --
recovery failed: read /dev/block/by-name/misc: No such file or directory (os error 2)
socket hidl2touchdaemon_client: the phone has no account named inet
/odm/bin/hw/vendor-oplus-hardware-touch-V2-hbp5-service registered no
vendor.oplus.hardware.touch.IOplusTouch/default in 20 seconds
/odm/bin/hw/android.hardware.weaver-service.nxp registered no
android.hardware.weaver.IWeaver/default in 20 seconds
```

结论：**SELinux、挂载、内核模块、USB 都不是问题**；两个真问题是：

1. **账号表缺 `inet`**：`skyward-sys` 内置的 Android 账号表只有 33 项，没有 `inet`(3003)，
   于是触摸服务的两个 socket 在 `chown root:inet` 这一步失败——socket 节点虽然建出来了（报告里 `sockets=yes`），
   但 fd 没交到服务手里（环境变量缺失），服务起来后一直不注册 → 没有触摸。
   （`everybody`、`sdcard_r`、`net_*` 同样缺，只是这次没用到。）
2. **`misc` 节点的时序**：recovery 服务第一次启动时 `/dev/block/by-name/misc` 还没被 ueventd 建出来，
   `recovery()` 直接失败、被 init 重启一次（第二次就正常了）。功能上无害，但每次开机多一轮重启和三行错误日志。

另外，报告的 `usb: … adbd=running udc=configured` 说明这一版的 adb **已经是配好的**（上一版失败是因为当时 permissive 还没生效）。

### 14.1 这一轮的改动

| 改动 | 位置 |
| --- | --- |
| 账号表补齐 Android AID：`sdcard_r`(1028)、`net_bt_admin/net_bt/inet/net_raw/net_admin/net_bw_stats/net_bw_acct`(3001–3007)、`everybody`(9997) | `crates/skyward-sys/src/ueventd.rs` |
| 读 misc 前最多等 5 秒，等 ueventd 把 `/dev/block/by-name/misc` 建出来 | `crates/skyward/src/main.rs` |
| 报告里"did not go"只留真正的错误行（不再把纯路径行带进来） | 同上 |

### 14.2 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   4246f50ab324dd66a7efa8f113d3f4f46a2224b0fab298aee9f5f3448af07f44
```

开机后报告页仍会出现。要看的两行：

- `services: touch=pid N`（不再是 `no`）→ 触摸服务活着，应该能点；
- `usb: … adbd=running udc=configured` → 此时电脑上 `adb devices` 应当能看到设备。

如果 `touch: service=yes` 但 `services: touch=no` 仍然出现，下一轮就从报告里看它是"执行失败"还是"注册失败"，
再决定是给服务补 `/data/vendor/touchconfig` 这类目录，还是把 OFox 那套二进制搬进 ramdisk。


---

## 15. "待三四分钟自己重启回 recovery"的排查（2026-11-10 第五轮）

用户实测：进 Skyward 后放着不动，约 3–4 分钟会自己重启回 recovery。可能的来源有三类，这一版同时做了**缓解**和**取证**：

| 假设 | 依据 | 这一版怎么处理 |
| --- | --- | --- |
| OPPO 的 `phoenix_recovery`（init 里标了 `critical`）在等同厂 recovery 的握手，超时后用 `sys.powerctl` 重启 | phoenix 的字符串里有 `sys.powerctl`、`reboot,`、倒计时线程和 `vendor.oplus.phoenix.recovery.cmd` | 用 **SIGSTOP 冻结**它（不是 `stop`：停掉一个 critical 服务反而可能被 init 当成崩溃而重启）。配置 `[device] freeze = ["phoenix_recovery"]` |
| 内核 panic/watchdog（例如 hbp 触摸栈在没有用户态服务时跑几分钟） | 触摸模块全部加载、但当时的版本里触摸服务没起来 | 报告里新增 `uptime`、`bootreason`、pstore 记录；触摸服务这次也会真正起来 |
| 只是 recovery 服务被 init 重启（不是整机重启） | 上一版报告里出现过 `recovery failed: read misc` 后 init 重启服务 | 报告第一行的 `uptime` 可以区分：整机重启 → uptime 很小；服务重启 → uptime 已经几百秒 |

### 15.1 报告新增内容

```
boot: reason=<ro.boot.bootreason> sys=<sys.boot.reason> uptime=<秒>
misc: <BCB 的 command 与 args>
phoenix: pid N / no   battery=<°C>
-- pstore: <文件列表> --
<console-ramoops 里含 panic/watchdog/reboot/thermal/shutdown 的最后几行>
```

`/sys/fs/pstore` 在 recovery 里默认没挂载，Skyward 现在会尝试 `mount pstore` 再读。

### 15.2 镜像（这一版同时包含 §14 的触摸修复）

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   30e1ec3094cba0612bb3609e7a3dcd9fbc0b09841c74af504cdb2de2a72cb9f9
```

开机后：

1. `services: touch=pid N` → 触摸服务起来了（§14 的 `inet` 修复）；
2. `phoenix: pid N` → 冻结完成（若显示 `no`，说明它本来就没在跑，那这条假设可以排除）；
3. 若仍然 3–4 分钟重启，**下一次开机报告**里的 `boot: reason=… uptime=…` 与 pstore 行直接告诉我们重启来源：
   - `uptime` 很小 + `reason=reboot,recovery` → 有程序主动 `sys.powerctl` 重启（phoenix 若已冻结就能排除）；
   - `reason=watchdog` 或 pstore 里有 panic → 内核问题，再看 pstore 的调用栈。


---

## 16. 搬 OrangeFox 的方案：把触摸栈打进 ramdisk（2026-11-10 第六轮）

上一轮把 `inet` 修好之后触摸仍不行。观察到两件事：

- recovery 本体是 **Android 16** 构建（`BP2A.250605.015`），而 Skyward 一路跑的是**已安装系统（Android 17）** 的 `/odm` 二进制；
- OFox 的 `recovery/root` 里自带一整套 **A16 构建**的触摸栈（服务 + `libolc_vnd`、`vendor.oplus.hardware.touch-V2-ndk`、`olc2`、`libtensorflowlite_oplus`、`libc++_v36`）。

A17 的厂商服务跑在 A16 的 linker/libc++ 上，ABI 不匹配就足以解释"进程起得来、但永远不注册/不出点"。

### 16.1 做法：`skyward-pack --overlay`

打包器新增 `--overlay <dir>`：把一个目录里的文件和符号链接按原路径加进 ramdisk（保留权限）。本次 overlay 内容（全部来自 OFox 树，A16）：

```
skyward/touch/bin/vendor-oplus-hardware-touch-V2-hbp5-service
skyward/touch/lib64/libolc_vnd.so
skyward/touch/lib64/vendor.oplus.hardware.touch-V2-ndk.so
skyward/touch/lib64/vendor.oplus.hardware.olc2-V2-ndk.so
skyward/touch/lib64/libtensorflowlite_oplus.so
skyward/touch/lib64/libc++_v36.so
```

配置里触摸服务改为从 ramdisk 启动：

```toml
[[service]]
path = "/skyward/touch/bin/vendor-oplus-hardware-touch-V2-hbp5-service"
libraries = ["/skyward/touch/lib64", "/odm/lib64", "/vendor/lib64", "/system/lib64"]
ready_service = "vendor.oplus.hardware.touch.IOplusTouch/default"
capabilities = ["SYS_NICE"]
user = "root"
groups = ["root", "system"]
sockets = [
  { name = "hidl2touchdaemon_client", kind = "dgram", mode = 0o660, user = "root", group = "inet" },
  { name = "hidl2touchdaemon_server", kind = "dgram", mode = 0o660, user = "root", group = "inet" },
]
```

固件仍用已装系统的 `/odm/firmware/tp/fairlady`（那是数据，不是 ABI）；服务清单仍由 `/odm/etc/vintf/manifest/manifest_touch_aidl.xml` 提供，Skyward 的 `publish_descriptions` 会照常发布。

报告里的 `touch:` / `services:` 两行现在会显示**配置里的实际路径**，便于确认加载的是 ramdisk 版还是 `/odm` 版。

### 16.2 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   41bddb83c402d5e06f2119374981f21c010c9456dc9b133bddcde33cafd51871
ramdisk  72,253,421 字节（比上一版多 4.9 MB 的 A16 触摸栈）
```

开机后报告里应能看到：

```
touch: service=yes at=/skyward/touch/bin/vendor-oplus-hardware-touch-V2-hbp5-service hbp_core=yes hbp0=yes sockets=yes
services: touch=pid N …
```

如果 `touch=pid N` 但仍不出点，下一轮就把 A16 的固件目录也打进 overlay，并加一个 `[[bind]]` 把 `/odm/firmware` 指向它（固件与配置 XML 的版本错配也是可能的）。


---

## 17. 让触摸程序自己说话（2026-11-10 第七轮）

把 OFox 的 A16 触摸栈打进 ramdisk 之后，报告里仍然是 `services: touch=no`——程序没能留在运行状态。静态检查过一遍 ELF 依赖闭包（`libolc_vnd`、`vendor.oplus.hardware.touch-V2-ndk`、`olc2`、`libtensorflowlite_oplus`、`libc++_v36` 及其传递依赖），在「overlay + recovery 的 /system/lib64 + OFox 的 vendor/odm lib64」里**没有缺库**，所以问题只可能在运行时。

这一版让它把话说出来：

| 改动 | 位置 |
| --- | --- |
| 每个服务的 stdout/stderr 不再丢到 `/dev/null`，写进 `/skyward/run/<程序名>.log`（追加） | `crates/skyward/src/services.rs` |
| 开机报告新增 `-- <程序名> said (N lines) --`，给出该日志最后 10 行 | `crates/skyward/src/main.rs` |
| 打包器 `--overlay` 现在也带目录；overlay 里加了服务要写的 `/data/vendor/touchconfig`、`/data/vendor/touch`（init 的 rc 会在开机时建，recovery 里没人建） | `crates/skyward-pack`、`crates/skyward-bootimg`（新增 `Ramdisk::mkdir`） |

触摸程序用到的路径（来自它自己的字符串）也一并记录：

```
/odm/firmware/tp/        ← 已装系统的固件（A17 数据，通常兼容）
/vendor/firmware/tp/     ← 回退
/data/vendor/touchconfig/custom_cfg.cache
/data/vendor/touch/{record,upload,simulate}
/proc/touchpanel/*
```

### 17.1 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   75d519766849be2ede2cd251dfcefbe591f2fa7e61462a1eea9b901d83c838c9
ramdisk  72,254,287 字节
```

### 17.2 报告里会多出的一段

```
-- vendor-oplus-hardware-touch-V2-hbp5-service said (N lines) --
<它自己的报错，最后 10 行，每行截到 100 字>
```

可能的几种读数与下一步：

| 读数 | 含义 | 下一步 |
| --- | --- | --- |
| 一行都没有 | 进程根本没 exec 成功 | 看 `-- kernel log --` 里的 `CANNOT LINK EXECUTABLE` / `exec` 错误，补库或改路径 |
| `dlopen failed: ...` | 缺某个运行时 dlopen 的库 | 从 OFox 树/已装系统补进 overlay |
| `Can't find olc2 service` / binder 相关 | 需要 `vendor.oplus.hardware.olc2` 服务 | 在配置里补一个 `[[service]]` 启动 olc2 |
| 找不到 `/odm/firmware/tp/...` 或解析失败 | 固件/配置版本不匹配 | 把 A16 的 `odm/firmware/tp/fairlady` 也打进 overlay，并加 `[[bind]]` 让 `/odm/firmware` 指向它 |
| `Permission denied` 打开 `/proc/touchpanel/*` | 节点属主/权限或 SELinux | 目前 permissive，多半是属主：用 `[[owners]]` 给 `/proc/touchpanel` 与 `/dev/hbp*` 设属主 |


---

## 18. 触摸程序的链接失败：库搜索顺序（2026-11-10 第八轮）

真机日志（这一版新增的服务日志把原因直接说出来了）：

```
CANNOT LINK EXECUTABLE "/skyward/touch/bin/vendor-oplus-hardware-touch-V2-hbp5-service":
library "libapexsupport.so" not found: needed by /vendor/lib64/libbinder.so in namespace (default)
```

原因不是缺库，而是**库搜索顺序**：触摸服务的 `libraries` 里写了
`["/skyward/touch/lib64", "/odm/lib64", "/vendor/lib64", "/system/lib64"]`，
于是链接器把 `libbinder.so` 解析成了**已装系统 A17 的 `/vendor/lib64/libbinder.so`**，
而它依赖 APEX 里的 `libapexsupport.so`（recovery 里没有）。

把依赖闭包按「overlay + recovery 自己的 `/system/lib64`」重算了一遍：全部库都能解析到，
`/vendor/lib64`、`/odm/lib64` 根本不是这个程序需要的。修正为：

```toml
libraries = ["/skyward/touch/lib64", "/system/lib64"]
```

规则：**ramdisk 里跑的程序就只该用 ramdisk 的库**；把已装系统的 `vendor/odm/lib64` 放进
`LD_LIBRARY_PATH` 会让 A16 的程序链到 A17 的库上，`libapexsupport` 只是第一个撞上的症状。

### 18.1 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   123fc01c5c00f67987ab7b2867b05e95be91749284782ed868f7cd73d6a01fee
```

下一步看报告：`services: touch=pid N` 且无新报错就说明链接通过了；
若日志里出现固件/olc2/权限相关的新错误，按 §17.2 的对照表处理。


---

## 19. 触摸之后：圆角与 adb（2026-11-10 第九轮）

**触摸已经能用**（§17/§18 的 A16 触摸栈 + 只链 recovery 自己的 `/system/lib64`）。剩下两件事：

### 19.1 屏幕圆角

用户照片里可以直接量：紫色页面的圆角就是配置里的 120 px，在照片中约 56 图像像素；
物理屏幕的圆角约 87 图像像素。按同一比例换算，物理圆角 ≈ 186 设备像素。
因此把 `[display] corner_radius` 从 120 调到 **180**（仍可按观感微调）。

### 19.2 adb：`adbd` 在跑，Mac 认不到

报告里 `usb: config=adb state=adb adbd=running udc=configured` —— 说明**主机已经枚举并配置了设备**，
所以问题不在"没挂上"，而在 adb 接口/adbd 服务这一层。这一版加了两件事：

| 改动 | 目的 |
| --- | --- |
| 报告新增 `gadget: vid=… pid=… udc=… f1=<functions/ffs.adb|none> functions=[…] serial=…` | 看 init 有没有把 adb 功能链进配置、USB 产品号对不对 |
| 报告新增 `ffs_ready=<sys.usb.ffs.ready> ep0=<yes/no>` | `ep0` 出现才说明 adbd 真的打开了 functionfs；这是"主机能看到接口但 adb 连不上"的关键判据 |
| 启动 2 秒后若 `ffs_ready != 1`，自动 `ctl.restart adbd` | 修 adbd 与 init 设置 gadget 的时序竞争 |

主机侧需要两条命令确认：

```sh
adb kill-server && adb devices
system_profiler SPUSBDataType | grep -iE "oneplus|android|18d1|adb" -B2 -A8
```

判读：

| 现象 | 含义 | 下一步 |
| --- | --- | --- |
| `system_profiler` 里没有 18d1 设备 | 主机没枚举到 | 检查线/口、`state` 是否真为 configured |
| 有 18d1 设备但 `adb devices` 空 | 接口在、adbd 没在 ffs 上服务 | 看报告 `ffs_ready`/`ep0`，必要时重启 adbd |
| `adb devices` 显示 `unauthorized` | 需要授权 | `ro.adb.secure` 没生效，检查 `prop.default` 覆盖顺序 |

### 19.3 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   391d42041f826b237f679e6b3f1a7de933a7d875359bdb116f54b7013d1cdfce
配置      [display] corner_radius = 180；触摸栈仍随 ramdisk 走（A16）
```


---

## 20. adb 的下一步：USB 端口方向（2026-11-10 第十轮）

报告（§19 版）：

```
gadget: vid=0x18d1 pid=0xd001 udc=a600000.dwc3
        f1=../../../../usb_gadget/g1/functions/ffs.adb functions=[ffs.adb,ffs.fastboot,ffs.mtp]
        serial=Some("3B162B0073V00000")
usb gadget: ffs_mounted=true configfs=yes usb_f_fs=NO libcomposite=NO ffs_ready=Some("1") ep0=yes
```

主机侧：`adb devices` 空，`system_profiler SPUSBDataType` 里**根本没有 18d1 设备**。

结论：手机侧 gadget 已绑定、adbd 已打开 functionfs（`ep0` 在），但**端口没有以"从设备"身份被主机枚举**。
最可疑的是 ColorOS recovery 的 `on fs` 里那句：

```
write /sys/class/oplus_chg/usb/otg_switch 1
```

这正是 OPPO recovery 给"插 U 盘"准备的主机模式；主机模式下 Mac 的 USB 总线上什么也看不到。
（`udc=configured` 是早期枚举留下的旧状态，端口翻成主机模式后设备侧掉了，但 sysfs 的值不会立刻回落到 `not attached`。）

### 20.1 这一版

1. 在向 init 要 adb 之前，先把端口写回设备模式：
   ```
   /sys/class/oplus_chg/usb/otg_switch          0
   /sys/bus/platform/devices/a600000.dwc3/mode  peripheral
   /sys/bus/platform/devices/a600000.ssusb/mode peripheral   （存在才写）
   ```
   随后 `usb::switch("adb")` 会解绑再绑定 UDC，让主机重新枚举。
2. 报告新增一行端口状态：
   ```
   port: otg_switch=0 typec_role=device udc=[a600000.dwc3=configured,...]
   ```
   三个值一起看就知道端口到底站在哪一边。

### 20.2 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   b646db674868f09bb143ac59933d55f6f364b2cc57d3ea8890c220ead69caee3
```

判断依据：

| 报告 `port:` | 主机侧 | 结论 |
| --- | --- | --- |
| `otg_switch=0 typec_role=device` 且 udc=configured | 应能看到 18d1 | 成功 |
| `otg_switch=1` | 看不到 | 写入未生效或被 init 的 `on fs` 又改回去 → 下次改为在 Skyward 起来后周期性重写 |
| `typec_role=source` | 看不到 | 需要走 typec 角色切换（`/sys/class/typec/port0/role`） |
| `otg_switch=0` 但 udc 不是 configured | 看不到 | UDC 没绑上，改回用 init 的 `sys.usb.config` 触发 |


---

## 21. adb 找到了：端口要"晚一点再交一次"（2026-11-10 第十一轮）

决定性线索来自用户：**打开 Skyward 的 ADB sideload 页面后，Mac 立刻就能 `adb devices` 看到设备（状态 `sideload`）**。

也就是说：端口、gadget、adbd、Mac 的 USB 栈全都没问题。差别只有一个——**sideload 页面是"后来"把端口
解绑再绑定的**（`sys.usb.config=none` → `sideload`，init 因此重写 UDC），而开机时的 adb 是在 init
还没把端口交出来之前绑的：那一次绑定主机从未真正枚举过，之后 `sys.usb.state` 即使写着 `adb`、`udc`
也写着 `configured`，都只是残留状态。

### 21.1 修法

在开机 2 秒后（等 init 的 `on fs` 把端口的事做完），**照 sideload 页面的做法把 adb 也解绑再绑一遍**：

```rust
let _ = skyward_sys::usb::switch("adb", Duration::from_secs(5));   // none → adb
```

失败则退回只写 `sys.usb.config=adb`，并把原因写进内核日志/开机报告。

### 21.2 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   b121653783c0274cf422fb2d5ea3190b2e6b31fadbf62177fafbb063bef03d1f
```

验证方式：刷完插在 Mac 上，`adb devices` 应出现一行（状态应为 `device`，不再是 `sideload`）；
手机报告里 `usb: … state=adb adbd=running udc=configured`、`ffs_ready=1`、`ep0=yes`。

若仍不出现，可在 Skyward 的 Terminal 里手动复现同一条路径确认：

```
setprop sys.usb.config none ; sleep 1 ; setprop sys.usb.config adb
```

能出现就说明只是时序，我会把重绑的等待时间/次数调整；不能出现则要看 `port:` 那行的 `typec_role`。


---

## 22. adb 的真正原因：ColorOS 的 recovery init 不认识 `mtp,adb`（2026-11-10 第十二轮）

用户实测：开机后 `adb devices` 空，但在 Skyward 的 Terminal 里 `setprop sys.usb.config adb` 之后就能看到设备（状态 `recovery`）。

这句话把范围锁死了——**属性最终不是 `adb`**。查 stock ramdisk 的 `init.rc`：

```
on property:sys.usb.config=adb        ← 有
on property:sys.usb.config=sideload   ← 有
on property:sys.usb.config=fastboot   ← 有
on property:sys.usb.config=none       ← 有
mtp                                   ← 一处都没有（grep "mtp" 零命中）
```

而 Skyward 的文件传输功能在设置读回之后会**主动把端口切到 `mtp,adb`**（`usb::switch(resting())`，
`resting()` 在文件传输开着时返回 `mtp,adb`）。init 没有这个值对应的触发块 → 端口不被设置 → 主机什么也看不到。
`sideload` 页面能用，正是因为 `sideload` 在 init 的触发块表里。

### 22.1 修法

新配置 `[device] mtp`（默认 `true`）：

- `mtp = false` 时，Skyward 不启动文件传输，`resting()` 只返回 `adb`，因此永远不会写 `mtp,adb`；
- 用户去开文件传输开关时，会提示"这台手机的 recovery 不提供文件传输"，而不是把端口弄坏。

fairlady 配置：

```toml
[device]
mtp = false
```

### 22.2 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   17cdc6d93ba233e03aa15fcc19261d1419c0de5b6d8e4635bd9b746646aa70d9
```

验证：刷完开机，插上 Mac，`adb devices` 应直接出现设备（`recovery`），不需要再手动 `setprop`。

代价：**文件传输（MTP）在这台机器上暂时不可用**。要做的话有两条路：
1. 在 ramdisk 的 init 里补 `mtp,adb` 的处理块（需要能改 init.rc，属于 §5-B 那类"预处理 stock"）；
2. Skyward 自己按 configfs 建 MTP function（`ffs.mtp` 已在 gadget 里），不依赖 init。


---

## 23. 屏幕圆角的精确值：160 px（2026-11-10 第十三轮）

先查了能离线拿到的所有地方，都不是这台机器的真值：

- `framework-res.apk` 的 `config_mainDisplayShape` 是 `1080×2400、圆角 20px` 的路径（`/vendor/overlay/FrameworksResTarget_Vendor.apk` 同值），与 1216×2640 面板不符，是 vendor 镜像里的通用默认；
- `rounded_corner_radius` = 0dp（`_adjustment` 也是 0）；
- `/vendor/etc/displayconfig/display_id_*.xml` 只有亮度曲线，`/odm/etc/display` 只有 qdcm 校准，`dtbo`/`vendor_boot` 里没有任何 `round/corner` 属性；
- ColorOS 的截图是完整矩形，不带圆角遮罩，无法从截图测量。

真值在 **WindowManager** 里，ColorOS 下一条命令即可：

```sh
adb shell dumpsys window displays | grep -m1 mRoundedCorners
```

输出：

```
mRoundedCorners=RoundedCorners{[RoundedCorner{position=TopLeft, radius=160, center=Point(160, 160)},
  RoundedCorner{position=TopRight, radius=160, center=Point(1056, 160)},
  RoundedCorner{position=BottomRight, radius=160, center=Point(1056, 2480)},
  RoundedCorner{position=BottomLeft, radius=160, center=Point(160, 2480)}]}
mDisplayShape=DisplayShape{type=2, displayWidth=1216, displayHeight=2640, physicalPixelDisplaySizeRatio=1.0, ...}
```

四个角都是 **160 px**（显示空间 1216×2640）。`dumpsys SurfaceFlinger` 里那些 434/207/200 是各个窗口（如设置页滑入的面板）自己的圆角，不是屏幕圆角。

因此 `[display] corner_radius = 160`（之前按照片估的 180 偏大）。

### 23.1 镜像

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   6f46aecbe31b88f986dfdb6a6f9ede4bee8ff8b5e11dee79866967c9633bf17a
```


---

## 24. data 解密的进展与卡点（2026-11-10 第十四轮）

### 24.1 已经打通的部分

1. **元数据解密**：`/metadata/vold/metadata_encryption/key` 由 KeyMint 解出，`/data` 被 dm-crypt 映射成 `/dev/block/dm-18` 并挂载（rw, inlinecrypt）。
2. **文件系统检查**：recovery ramdisk 里的 `fsck.f2fs` 解释器是 `/system/bin/bootstrap/linker64`，ramdisk 没有它 →
   `execve` 返回 ENOENT。把该路径指向已装系统的 `/mnt/system/system/bin/bootstrap/linker64` 后 fsck 正常
   （已写进 `prepare_early_loader()`）。
3. **SP 文件的真实位置**：ColorOS 把合成密码放在 **`/data/system_de/<user>/spblob/`** 目录里：
   `<id>.spblob`、`<id>.pwd`、`<id>.weaver`（5 字节：布局 1 + slot 0）、`<id>.handle` 等。
   Skyward 的 `PROTECTORS = /data/system_de` 正好对应，读到了锁屏类型（说明 `.pwd` 解析正确）。
4. **服务链**：qseecomd、onekeymint(KeyMint v4)、gatekeeper、NFC HAL、secure_element(eSE)、weaver 全部起来并注册；
   顺带修好了 VINTF 清单匹配（展开写法 `<name>IWeaver</name><instance>default</instance>`）、
   `hw` 子目录库路径（`libEseUtils.so`）、`nxp-nci` 驱动与 `/dev/nq-nci` 属主、`persist` 挂载时序。

### 24.2 卡点：weaver 的 slot 读不出来

```
ask the weaver about itself: the service refused the call with its error 1 (Failed to retrieve slots info)
```

- ColorOS 17 的 NXP weaver（`ese_weaver.nxp.so`）**经 framework 的 OMAPI 服务**
  （`android.se.omapi.ISecureElementService`）访问 eSE；recovery 里没有 framework，因此 OMAPI 不存在，
  servicemanager 里也从未出现该服务（dmesg 只有 weaver 的 `find` 拒绝，没有 eSE 的 `add`）。
- 换成 OFox 树里 **A16 的 weaver** 也一样：它的 `ese_weaver.nxp.so` 虽然自带 transport，
  但仍链接 `android.se.omapi-V1-ndk.so`，同样找不到 OMAPI 服务。

也就是说：**weaver 的 slot 访问必须绕开 OMAPI**，否则拿不到 `.weaver` 记录的 slot 0 里的 secret。

### 24.3 可选的三条路

1. **自己实现一个最小 OMAPI 服务**：在 Skyward 里注册 `android.se.omapi.ISecureElementService/default`，
   `getReaders()` 返回 eSE reader、`openSession`/`transmit` 转给 eSE HAL 的 `ISecureElement/eSE1`。
   weaver 只用到 OMAPI 的 reader/session/transmit 几个调用，工作量中等（一个 crate 量级）。
2. **直接写 eSE applet 的 APDU 客户端**：读 `.weaver` 的 slot 0，按 NXP weaver applet 的帧格式
   （A16 lib 里能看到 `FrameGetSlotCmd`/`GetSlots` 的痕迹）取 secret，完全绕开 weaver HAL。
3. **先确认 OFox 在真机上是否真能解密**：如果它的 TWRP 在这台机器上能解开 `/data`，说明 TWRP 走的是
   另一条路（很可能是 gatekeeper + keymaster 直接解 user key，不碰 weaver），那就照它的做法做。

在拿到结论之前，这一版镜像的状态是：**除了解锁用户数据以外全部可用**（触摸、adb、背光、圆角、服务链）。


---

## 25. data 解密打通（2026-11-10 第十五轮）

### 25.1 关键：TWRP 的路线与我们的差别

刷入移植过 vbmeta 的 OrangeFox（`OrangeFox-R11.3-Unofficial-fairlady.img`，官方 OPN 签名的
`recovery.vbmeta` 换掉了它原来的无签名 vbmeta）后，OFox 能正常开机并**成功解密 /data**。
它的 `/tmp/recovery.log` 写明了路线：

```
Handle is 'bf0f76f0bdcbafc4'          ← 与 /data/system_de/0/spblob/ 里的文件名一致
Using synthetic password method
Is_Weaver → using weaver → Get_Weaver_Data → weaver key size is 16
Attempting to unwrap synthetic password blob (spblob v2/v3)
User 0 Decrypted Successfully!
```

服务：`keystore2`、`vendor.weaver_nxp`、`vendor.gatekeeper_default`、`qseecomd`、
`vendor.secure_element`（没有 NFC HAL）。OFox 的 ramdisk 里还带了一个
**`/system/bin/se_omapi`**——这是 recovery 里能用的 OMAPI 服务，ColorOS 的 recovery 没有。

### 25.2 我们缺的两样东西

1. **OMAPI 服务**：NXP weaver 的 `ese_weaver.nxp.so` 经 `android.se.omapi.ISecureElementService`
   访问 eSE。framework 提供的那份在 recovery 里不存在，OFox 自带 `se_omapi` 顶上。
2. **库不能混用**：把 OFox ramdisk 的 `libbinder.so`/`libbinder_ndk.so` 和 ColorOS recovery 自己的
   servicemanager 混在一起，会得到
   `Parcel: Expecting header 0x5245434f but found 0x53595354. Mixing copies of libbinder?`
   然后服务在注册时 `SIGABRT`。

### 25.3 解法

- 把 OFox 的 `se_omapi`（连同它需要的 A16 库）和 A16 weaver 栈一起打进 overlay：
  `skyward/omapi/{bin,lib64}`、`skyward/weaver/{bin,system-lib64,odm-lib64,vendor-lib64}`；
- **`libraries` 里 `/system/lib64` 必须排最前**，让 recovery 自己的 libbinder 生效，
  OFox 那套库只用来补 recovery 没有的部分（`libapexsupport`、`android.se.omapi-V1-ndk`、
  `android.hardware.weaver-V2-ndk`、`ese_weaver.nxp`、NXP 的 keymaster transport 等）；
- 配置顺序：`qseecomd → keymint → gatekeeper → NFC HAL → secure_element → se_omapi → weaver`。

验证（Skyward 日志）：

```
the users whose own files are still closed: [0]     ← 开机时等锁屏密码
the users whose own files are still closed: []      ← 输入 PIN 后全部解锁
/data/media/0 → Android DCIM Documents Download Fox Movies Music …
```

### 25.4 顺带修好的

- `fsck.f2fs` 的解释器 `/system/bin/bootstrap/linker64`（ramdisk 没有）→ `prepare_early_loader()`
  指向已装系统的 bootstrap linker；
- `[partitions] mounted = ["persist"]`：stock init 的 mount persist 会踩 ueventd 时序；
- `nxp-nci` 模块 + `/dev/nq-nci` 属主 `nfc:nfc` + NFC HAL（`android.hardware.nfc2-service.nxp`）；
- VINTF 清单的展开写法匹配（`<name>IWeaver</name><instance>default</instance>`）；
- `hw/` 子目录库路径（`libEseUtils.so` 在 `/vendor/lib64/hw`）；
- `[[owners]]` 遇到不存在的节点只记 kmsg，不再中断 bring-up。

## 26. 开机直接进解密页（2026-11-10 第十六轮）

`[device] diagnose`（当初 adb 坏掉时用来把自检报告显示在屏幕上）已从配置里去掉；
`App::data_waiting(Some(..))` 现在会直接打开解密页：

```rust
// A phone whose files wait for a screen lock shows the page that
// takes it, rather than the one the recovery would start on.
if waiting.is_some() && self.screen() != Screen::Decrypt {
    self.open(Screen::Decrypt);
}
```

镜像（当前）：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   76cf80b8e877102b5f790c6d8988eabd47c94e16b0fef82bbaec991fba8ab82a
```


---

## 27. 振动、MTP、以及"ColorOS 17"的显示（2026-11-10 第十七轮）

### 27.1 振动

- `qcom-hv-haptics` 的 `/sys/class/qcom-haptics/lra_frequency_hz` 在自家系统播放/校准前读出来是 **0**，
  于是 `Haptics::open` 报 `the motor swings at 0 Hz`；而这个错误当时是致命的，Skyward 直接退出 → **黑屏**。
- 真值在 dtbo：`qcom,lra-period-us = 5882µs` → **170 Hz**（`1e6 / 5882`）。
- 修法：`[haptics]` 增加可选 `frequency_hz`，驱动不说时用它；并且马达打不开只写 kmsg、不再中断启动：

```rust
let mut haptics = match cfg.haptics.as_ref().map(Haptics::open).transpose() {
    Ok(haptics) => haptics,
    Err(e) => { skyward_sys::kmsg(&format!("the vibration motor is left out: {e:#}"))?; None }
};
```

### 27.2 MTP

- Skyward 自带 `system/etc/init/skyward-mtp.rc`，在 `sys.usb.config=mtp,adb` 时把 `ffs.mtp` 与 `ffs.adb`
  一起绑上（idProduct 0x4EE2）。ColorOS 的 recovery `init.rc` 只认 `none/adb/sideload/fastboot`，
  没有 `mtp,adb`，所以要靠这份 rc。
- 原来的 rc 里启动 adbd 的条件是 `property:sys.usb.config_disable=0`，**ColorOS 从不设这个属性**，
  于是 adbd 没被拉起 → `sys.usb.ffs.ready` 停在 0 → 端口永远到不了 `mtp,adb`，连 adb 一起没了。
  已改成无条件 `start adbd`。
- Skyward 侧加兜底：`usb::switch_resting()`，`mtp,adb` 起不来时自动退回 `adb`，避免"端口全丢"。

### 27.3 ColorOS 版本

- 系统的版本信息在 **`my_product` / `my_manifest` 分区根目录的 `build.prop`** 里：
  `ro.build.version.oplusrom.display=17.0.1`、`ro.build.display.id.show=PLZ110_17.0.1.200(CN01)`。
- `skyward_info::installed_props` 原来对非 system/vendor 分区只读 `etc/build.prop`，读不到这些；
  已改成同时读根目录的 `build.prop`。
- `skyward_systems::installed_name` 增加 ColorOS 分支：有 `ro.build.version.oplusrom.display` 时
  叫 **ColorOS <主版本>**（17.0.1 → ColorOS 17），版本优先用 `ro.build.display.id.show`。

镜像（当前）：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   74ff91988b8296b8088c2c8c9fb1dafbdfc4fee3ee5637088d4ddd8075872685
```


### 27.4 MTP 打通（验证）

踩到的坑：`skyward-pack` 用 `include_bytes!("mtp.rc")` 把 rc **编进打包器本身**，
只改 `crates/skyward-pack/src/mtp.rc` 而不重编 `skyward-pack`，打出来的镜像里仍是旧 rc。
所以改 rc 之后必须先 `cargo build --release -p skyward-pack`。

修好后的状态：

```
sys.usb.state = mtp,adb
gadget: f1 → ffs.mtp, f2 → ffs.adb，idProduct 0x4ee2，configuration "mtp_adb"
UDC: configured
内核日志: mtp: the computer configured the function
```

Mac 侧用 libmtp 验证（macOS 本身不挂载 MTP，需要 Android File Transfer / OpenMTP / libmtp 这类客户端）：

```
$ mtp-detect
Device 0 (VID=18d1 and PID=4ee2) is a Google Inc Nexus/Pixel (MTP+ADB).
Device info:
   Manufacturer: OnePlus
   Model: OnePlus 15T
   Device version: Skyward 0.2.0
   Serial number: 3B162B0073V00000
$ mtp-files
Listing File Information on Device with name: OnePlus 15T [SN:3B162B0073V00000]
```

镜像（当前）：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   7c51b0a6220df6acdcdb7a49be7f6e37081ad0382be79ded9fa0e966a495d21f
```


---

## 28. 预测性返回手势（2026-11-10 第十八轮）

原来的边缘返回只是"划够 56dp 就返回"，配一个小箭头提示，页面不跟手。现在改成预测性返回：

- `Preview { front, behind, along, settle }`：从边缘按下并越过 slop 时，把当前页的画面留一份（`front`），
  并把"上一页"的画面（`backdrop`）放到下面；手指移动时 `along = travel / 屏幕宽`，
  用 `Canvas::slide` 把两页合成——页面跟手、下面露出上一页，和系统的手势一致；
- 松手：过阈值就走 `back()`，并把 `Turn` 的起点设在手指离开的位置（`Spring::new(now, (along-1)*1000, …)`），
  所以动画从当前位置接着走，不会跳回 0；不到阈值则用弹簧把 `along` 送回 0（`preview_settle`），
  页面回到原处，小箭头照旧下沉；
- `tick` 里清掉已经回到位的预览；`deadline` 在预览收尾期间继续要帧；
- 露出的"上一页"用 `backdrop`，但会核对它保存的是不是 `stack[len-2]` 那一页
  （连续返回后 `backdrop` 会是刚离开的那页，不是下面那页，此时只铺页面底色）——
  `Turn` 因此多带一个 `left_screen`。

测试：`a_swipe_from_the_edge_takes_the_page_with_the_finger`、
`a_long_swipe_from_the_edge_carries_the_page_out`，`skyward-ui` 336 个测试全过。

镜像（当前）：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   cc869d8933aff3367970000b0ce334bc6d6009de6582f9898ef86df37417fa85
```


### 28.1 修正：取消手势后"上一页变白/黑 + 当前页卡一下"

原因：`begin_preview` 用 `take()` 把 app 留着的两份画布拿走了（`layer` 是当前页、`backdrop` 是上一页），
手势取消后没有还回去，于是

- 下一次滑动时没有"上一页"可画，只能铺页面底色（浅色模式白、深色模式黑）；
- 当前页的画布也没了，要整页重画一次，看起来就是卡一下。

改法：**只借用、不取走**——`Preview.front`/`behind` 改成 `Option<Canvas>`，只有 app 自己也没有可用画布时
才现画一份；走 `back()` 时再在那一刻取 `layer` 给 `Turn.left`。顺带在滑动期间不再每帧 `render()` 整页
（页面已经冻结，没必要重画），拖动更顺。

回归测试：`a_swipe_let_go_of_keeps_the_page_under_it_for_the_next`（取消后 `backdrop` 仍在，
下一次预览的 `behind` 为 `None`，说明用的是留着的画布而不是现画的底色）。

镜像：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   2170243a6d06cc63a5080d9d8df96c4e20a9cb952b0d1974e767ce33dc66b294
```


### 28.2 三个动画/预览问题的修正

1. **进入动画中点目标页，动画直接结束**：`touch()` 的 `Touch::Down` 分支无条件调用了 `end_turn()`
   （本意是让滑动的页面停下），结果手指一碰就把入场动画掐断。已去掉，手指不再打断页面滑动；
   测试改成 `a_finger_leaves_a_sliding_page_to_finish_and_a_page_never_shown_gives_way_at_once`。
2. **入场动画没播完就返回，退场从屏幕最左开始**：`leave(true)` 会把正在播放的 `Turn` 丢掉，
   新 `Turn` 的弹簧从边缘（-1000）开始。现在 `Turn` 记一个 `last`（上一帧走到哪里，0~1），
   退场弹簧从 `(last - 1) * 1000` 起，动画接着当前位置走。
   测试：`a_page_turned_back_while_it_opens_comes_back_from_where_it_is`。
3. **三级页返回主页时预览白屏**：原来只留了"上一次离开的那一页"一份画面（`backdrop`），
   三级页时对不上就退化成页面底色。改成留**一摞**页面画面 `lefts: Vec<(Screen, Canvas)>`
   （最多 3 份，每份一屏 ≈12.8MB）：前进的滑动落定后把离开的页压栈，返回时弹栈；
   预览取 `lefts` 的栈顶作为"下面那页"。
   测试：`a_swipe_three_pages_deep_shows_the_page_under_it`。

`skyward-ui` 339 个测试全过。镜像：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   326b7f7f3657fef38034e8de1394324f8385d3f8586623264b8e06808337b613
```


### 28.3 panic（黑屏 → 回主页）的定位与修正

加了 panic hook 后抓到了原因：

```
skyward panicked: panicked at crates/skyward-ui/src/app.rs:4342:41
init: Service 'recovery' (pid 501) received signal 6
```

`4342` 是 `compose_page` 里的 `let title = self.frame.as_ref().expect("rendered above").title;`。
为了拖动更顺，滑动期间跳过了 `render()`；但如果这期间页面被 `refresh()` 清掉（`frame = None`），
下一帧仍然跳过渲染，于是这里 `expect` 直接 abort。

修法：

- 页面被清掉就重新渲染：`if self.preview.is_none() || self.frame.is_none() { self.render(..) }`；
- 没有画面可滑的手势干脆不做预览（`begin_preview` 在 `layer` 与 `frame` 都没有时直接返回）；
- 真到了"无画面可滑"的情况，照常画当前页，而不是留空。

回归测试：`a_page_changed_while_a_swipe_holds_it_is_drawn_again`（预览中 `refresh()` 再 `compose` 不 panic）。

镜像：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   1e8830b9b76fea603bfc68d0e8eefdbad39b38d65679b4a428894d76bad063a4
```


### 28.4 第二个 panic：长列表补画行

```
skyward panicked: panicked at crates/skyward-ui/src/app.rs:4542:51
```

`paint_page` 给长列表补画一行时 `fresh.expect("a page drawn again as it last showed has its rows in view drawn")`：
`fresh` 为 `None`（"按上次显示的样子画页"）而那一行的缓存又不在时就会 abort。预览的 `begin_preview`
正好走这条路（它从 `shown` 画当前页，长列表滚过之后行缓存可能已经被回收）。

修法：

1. 预览改由 `compose` 来画：手势只置 `preview_wanted = Some(走过的距离)`，`compose_page` 里再
   `begin_preview(f, info)` 并用 `Some((f, info))` 画页，行不够就现画；
2. `paint_page` 里那处 `expect` 换成 `let Some((f, info)) = fresh else { continue }`——
   缺一行就让它保持页面底色，而不是把 recovery 弄挂。

镜像：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   539e4294210658d651e179186fe7b22d81745392e8b3c380f25689ca72726ff6
```


### 28.5 返回不离开当前页时不做预览

文件管理里返回是"爬上一级目录"（同一个 `Screen::Files`），`Choose` 页面是"回到这个位置的顶层"，
编辑器有未保存修改时返回只是提示，`WlanAdd` 的列表先收起——这些情况下"下面的页面"并不是返回目标，
预览就会显示成"打开文件管理的那个页面"。加了 `back_stays(now)` 判断，这些情况只显示小箭头、不做预览：

```rust
fn back_stays(&self, now: u64) -> bool {
    match self.screen() {
        Screen::Files => self.state.files.up().is_some(),
        Screen::Choose(place) => self.state.folder(Screen::Choose(place)).up().is_some(),
        Screen::Editor => self.state.editor.as_ref().is_some_and(|e| e.changed)
            && self.unsaved_warned.is_none_or(|at| now >= at + TOAST_MS),
        Screen::WlanAdd => self.state.adding.as_ref().is_some_and(|a| a.choosing),
        _ => false,
    }
}
```

到了文件管理的根目录（`up()` 为 `None`）返回才会离开页面，那时预览照常显示下面的页面。
测试：`a_swipe_where_back_stays_on_the_page_shows_no_preview`。

镜像：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   342a66267b3377053dab54aa46dbf7cd23cf4c7392d84b90e8ccc21ce6adae27
```


---

## 29. 开机动画（2026-11-10 第十九轮）

`prepare()`（挂载分区、起服务）实测有十几秒，屏幕一直是黑的。现在在 `prepare()` 之前就把屏幕点亮，
播一个开机动画，页面起来后交还：

- `crates/skyward-ui/src/boot.rs`：`frame(w, h, t, &mark)` 画一帧——深色底、
  **MiSans 的 "Skyward" 字标**（和 About 页同一个 `widgets::wordmark`，`Fonts.display` 那个字面）
  从暗处浮起并微微放大，下面是**一条填充的进度条**（`1 - exp(-t/5)`，永远填不满，页面接管时消失）；
- `crates/skyward/src/boot.rs`：`start(density)` 起一个线程，打开 `KmsSurface` 和背光（0.45），
  每 16ms 画一帧；`Booting` 的 `Drop` 会 join 线程、释放 surface；
- `main.rs::recovery()` 里在 `prepare()` 前 `let booting = boot::start(cfg.display.density);`，
  页面启动前 `drop(booting)`，保证两个 KMS surface 不会同时开着。

镜像：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   d4bcf3fce8e816b6b0a4b2b9eb8168d099a7fa1b0a4dab9203875be9fe606e89
```


---

## 30. 自动深浅色（2026-11-10 第二十轮）

设置页在"浅色 / 深色"选择器下面多了一行开关 **自动深浅色**，并带一行说明
"19:00 到 7:00 用深色，其余时间用浅色"。

- `State.auto_dark` / `State.local_hour`，开关随设置一起存（`settings.json` 的 `auto_dark`）；
- 状态栏每秒都会算一次本地时间（`fmt::clock`），现在这个时间也交给页面：
  `app.set_local_time("20:15")` → 小时变了才判断一次，`hour >= 19 || hour < 7` 就切深色，
  其余切浅色，只在真的变了才 `refresh()`；
- 开关打开的那一刻也会按最后一次知道的小时立即生效；
- `Switch::AutoDark` 走既有的 `Action::Toggle` 通道。

测试：`the_pages_go_dark_by_the_clock_when_asked`、`the_switch_turns_the_pages_dark_by_the_clock_at_once`，
`skyward-ui` 344 个全过。

镜像：

```
/tmp/fairlady/skyward-fairlady-vbmeta.img   104,857,600 字节
SHA-256   5cb321f38954ddbc7e4b01d81eaabba7bd48555a117f560278ca0cea5fb3b92f
```
