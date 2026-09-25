# AI Code Review 处理报告

- 来源：`aicodereview/[Flow] AI Review_ main_dev -_ main (#1) · rust_ecshop · Codeup.mhtml`
  （云效 Codeup AI 助手对 main_dev → main PR #1 的评审，已转换为 [ai_review_main_dev.md](ai_review_main_dev.md)）
- 评审范围：基础设施提交 `33c87a2`（CI workflow、配置、错误、SQLite、加密、鉴权、工具函数）
- 处理日期：2026-09-25
- 结论：**5 个代码问题中 5 个全部修复**；2 个架构建议中 1 个部分采纳、1 个暂不修复（理由见下）。

---

## 一、代码问题处理明细

### 1. CI 安装了 clippy 组件但未执行静态检查
- **级别**：🟡 Major ｜ **文件**：`.github/workflows/ci-main_dev.yml`（ci.yml 同样存在）
- **评审意见**：通过 `components: clippy` 安装了 clippy，但后续只运行 check/build/test，未运行 clippy，浪费安装时间且失去静态分析质量保障。建议增加 `cargo clippy --all-targets -- -D warnings`。
- **决定**：✅ **已修复**
- **处理**：`ci.yml` 与 `ci-main_dev.yml` 均在 Check 之后、Build 之前增加 `Clippy` 步骤：
  `cargo clippy --all-targets -- -D warnings`。
  为使 `-D warnings` 通过，同步清理了现存 clippy 告警：
  - `shared/util.rs`：`Some(s) if s.is_empty()` 冗余守卫 → `None | Some("")`
  - `http/routes/order.rs`：13 元组闭包返回类型抽为 `type MergeOrderRow`
  - `http/routes/marketing.rs`：8 元组抽为 `type PackageRow`
  - `http/routes/account.rs`：8 元组抽为 `type BonusRow`
  - `app/mod.rs`：`anyhow::Error` 的无效转换（clippy --fix 自动）
  - `domain/mod.rs`：`Money::to_decimal_string(&self)` → `to_decimal_string(self)`（Copy 类型）

### 2. 支付回调密钥不应使用硬编码的默认值
- **级别**：🟠 Critical / Security ｜ **文件**：`src/http/state.rs` L23
- **评审意见**：`payment_callback_secret` 默认硬编码 `"dev-payment-callback-secret"`，生产忘记设置环境变量时回退到已知默认值，攻击者可伪造支付回调。
- **决定**：✅ **已修复（采用启动校验方案，而非完全移除默认值）**
- **处理**：
  - 新增 `AppConfig::validate()`：当 `ECSHOP_ENV=production` 且密钥为空或等于开发默认值时，启动即报错拒绝运行（fail-fast）；
  - `app::run()` 启动时调用校验；
  - 开发环境保留默认值，保证 `cargo run` 开箱即用（docs/06 的 curl 验收流程依赖已知 secret）。
- **说明**：评审建议的“默认空字符串”会让本地开发/CI 每次都需手工注入密钥，成本偏高；启动校验 + 环境分支达到了同等的生产安全效果。已加单元测试覆盖 dev/production 两条路径。

### 3. 新增的认证逻辑缺少单元测试覆盖
- **级别**：建议 ｜ **文件**：`src/http/auth.rs`
- **评审意见**：Bearer 解析、会话查询等认证核心逻辑无测试。
- **决定**：✅ **已修复**
- **处理**：
  - 将 SQL 会话查询从 `authenticate` 中抽出为 `find_session_user(db, token_hash)`（可测），
    Bearer 解析抽出为 `extract_bearer_token(headers)`（可测）；
  - 新增 3 个测试：内存 SQLite + 真实 schema 下的会话命中/未命中、Bearer 头解析契约（缺失/非 Bearer/空 token/正常）；
  - 同时为 `crypto.rs` 补 5 个测试（密码哈希往返、随机盐、畸形哈希拒绝、SHA-256/HMAC 已知向量）。

### 4. PBKDF2 手动实现存在性能和安全隐患，应使用 pbkdf2 crate
- **级别**：🟠 Critical / Security ｜ **文件**：`src/infrastructure/crypto.rs` L55-72
- **评审意见**：手写 60,000 次迭代 PBKDF2 循环，性能低于优化实现，且未审计的自定义密码学实现有侧信道风险；建议换用 `pbkdf2` crate 的 `pbkdf2_hmac::<Sha256>`。
- **决定**：✅ **已修复**
- **处理**：
  - `Cargo.toml` 增加 `pbkdf2 = "0.12"`；
  - `pbkdf2_sha256` 改为调用 `pbkdf2::pbkdf2_hmac::<Sha256>`，删除手写 HMAC 迭代循环；
  - 存储格式（`pbkdf2$60000$salt$hash`）、盐长度、迭代次数不变，存量哈希仍可验证；
  - 保留 `constant_time_eq` 常量时间比较（评审原文亦认可该点）。

### 5. 修复负数金额格式化错误
- **级别**：🟠 Critical / Bugs ｜ **文件**：`src/shared/util.rs` L38-40
- **评审意见**：`cents_to_string(-150)` 用 `div_euclid(100)` 得 `-2`、`rem_euclid(100)` 得 `50`，输出 `"-2.50"`，正确应为 `"-1.50"`，影响退款/扣款场景。
- **决定**：✅ **已修复（但未照抄评审建议代码）**
- **处理**：
  ```rust
  let abs = cents.unsigned_abs();
  let sign = if cents < 0 { "-" } else { "" };
  format!("{}{}.{:02}", sign, abs / 100, abs % 100)
  ```
  - **注意**：评审给出的建议代码 `format!("{}{}.{}", ...)` 本身缺少 `{:02}` 零填充，`5` 分会被格式化为 `".5"` 而非 `".05"`，本次未采纳该细节，按正确实现修复；
  - 采用 `unsigned_abs()`，`i64::MIN` 不会因 `abs()` 溢出 panic；
  - 新增 4 组单元测试：正数、负数（含 `-150 → "-1.50"`）、`i64::MIN` 边界、`parse_money_cents` 非法输入拒绝。
  - 现状说明：当前调用方（`format_signed_cents` 等）均传入非负值，故线上无既有错误输出；此修复是防御性的。

---

## 二、架构设计建议处理

### 1. 认证模块与数据库 schema 的隐式耦合缺乏编译时保障
- **评审意见**：auth.rs 的 SQL 依赖 `include_str!` 嵌入的 schema，字段变更只会在运行时报错，无编译期保障。
- **决定**：❌ **暂不修复**
- **理由**：
  - 该耦合是“SQL 应用 + 嵌入式 schema”模式的固有属性，属于本次 PR 之外的架构演进问题；
  - Rust 生态的编译期方案（sqlx macro 需要真实数据库/离线缓存，diesel 需要引入完整 ORM）会显著改变基础设施选型，应在引入 MySQL 生产驱动时统一决策；
  - 现有缓解措施已存在：schema 与应用同仓库同提交，字段变更必然一起评审；本次 review 已为会话查询补齐单元测试（内存库 + 真实 schema），运行期回归可被 `cargo test` 捕获；
  - 已列入后续 MySQL/ORM 引入时的验收项，见下方“遗留事项”。

### 2. 支付回调密钥默认值与认证会话管理的安全边界模糊
- **评审意见**：多个安全关键点共享“依赖环境变量但提供不安全默认值”的模式。
- **决定**：✅ **部分采纳（随问题 #2 一并处理）**
- **处理**：支付回调密钥已有生产环境 fail-fast 校验（见问题 #2）；认证会话无“默认凭据”问题（token 为 32 字节随机数、库中只存 SHA-256 哈希），维持现状。
  “配置集中管理”的建议在引入 config crate/配置文件层时一并考虑，当前环境变量规模尚小。

---

## 三、验证结果

```text
cargo clippy --all-targets -- -D warnings   # 通过，0 警告
cargo test --all-targets                    # 13 passed, 0 failed
```

新增/变更测试一览：
- `shared::util::tests` ×4（正负金额、i64::MIN、money 解析）
- `infrastructure::crypto::tests` ×5（哈希往返、随机盐、畸形哈希、SHA-256/HMAC 向量）
- `http::auth::tests` ×3（会话命中/未命中、Bearer 解析契约）
- `http::state::tests` ×1（生产默认密钥拒绝）

## 四、遗留事项（后续 PR）

1. 引入 MySQL 生产驱动时，同步评估 sqlx/diesel 的编译期 schema 校验（架构建议 #1）。
2. 引入 config crate 时统一安全配置管理（架构建议 #2 剩余部分）。
3. 本地默认端口已改为 28080（工作区既有改动 `src/http/state.rs`，因本机 8080 被 Apache 占用），随本次一并提交。
