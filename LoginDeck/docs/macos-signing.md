# 旧版 Data Protection Keychain / 发行签名配置

> 当前默认已改为免费的 macOS 登录钥匙串，新密码不需要此配置。以下内容保留用于旧版存储访问和发行签名准备。

LoginDeck 使用 Data Protection Keychain 和 user-presence ACL。无团队身份的 ad-hoc 调试包在本机返回 OSStatus `-34018`；仅添加自造 entitlement 或重新做 ad-hoc 签名不能替代 Apple 授权。

## 准备


在本机钥匙串安装有效的 Apple Development 或 Developer ID Application 证书及对应私钥，并从 Apple Developer 获取匹配证书、团队和显式 App ID `com.autologin.desktop` 的 macOS provisioning profile，允许本应用的 Keychain access group。开发 profile 还应包含测试设备。签名身份本身不足以证明 profile、证书、设备匹配；最终必须运行实际签名包验证。

2026-09-15 本机 `security find-identity -v -p codesigning` 返回 0 个有效身份，因此当前无法完成实际签名和凭据验收。
mu q
## 生成与构建

仓库脚本只读取 profile 和签名身份，校验过期时间、App ID、团队和 Keychain group，生成最小 entitlement 与 Tauri 配置；不导入证书、不修改钥匙串、不执行签名。将下面的证书名称和 profile 路径替换成真实值，输出目录必须尚不存在：

```sh
python3 scripts/prepare-macos-signing.py \
  --identity 'Apple Development: YOUR NAME (YOUR TEAM)' \
  --profile /absolute/path/LoginDeck.provisionprofile \
  --output /private/tmp/logindeck-signing
pnpm --dir app exec tauri build --debug --bundles app \
  --config /private/tmp/logindeck-signing/tauri.signing.json
codesign --verify --deep --strict --verbose=2 target/debug/bundle/macos/LoginDeck.app
codesign -d --entitlements :- target/debug/bundle/macos/LoginDeck.app
```

Tauri 将 profile 放入包的 `Contents/embedded.provisionprofile`。不要将私钥、证书导出密码或个人 profile 提交到仓库。发布构建移除 `--debug`，按发行渠道另行完成公证；本脚本不自动完成发行签名、公证或 profile 的完整可信性验证。

## 实机验收

设置页的签名提示只检查声明，不能代替访问测试。完成网站和 Safari 专用测试账号的新增、重启读取、系统授权接受/取消、留空密码编辑、密码替换、删除及剪贴板清理。最后点击凭据清理重试，确认待清理数归零。若仍报错，记录数值 OSStatus；不要记录密码、凭据引用或 Keychain 查询内容。
