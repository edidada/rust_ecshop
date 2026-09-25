D:\develops\git\github\rust\axum-web-app
参考这个rust工程

php http server项目
添加git submodule
[submodule "third_party/shopex-ecshop"]
	path = third_party/shopex-ecshop
	url = https://gitee.com/softtomorrow/ecshop

git init
git remote add codeup mydada@mydada-cn-hangzhou.devops.aliyuncs.com:codeup/edidada/rust_ecshop.git
git remote add git@github.com:edidada/rust_ecshop.git
搭建骨架测试通过之后 编写github action yml，支持三个主流os

git push origin --all
git push codeup --all
然后按照url，编码，git add commit push两个远程仓库
注意不要编译测试，只要之前骨架编译通过就行
一个url git add commit push两个远程仓库一次
我稍后集中编译测试

# 这条建议的意思

## 先说背景：Rust 的依赖版本是"每个 crate 各算各的"

Rust（Cargo）允许多个**不兼容**的版本**同时存在**于一个项目的依赖树里。比如你项目里可以同时有 `sha2 0.9` 和 `sha2 0.10`，编译器不报错，两个都会编进去。

这跟很多语言不一样（比如 npm 可以靠 lockfile 强行统一，Python 一个环境通常只有一个版本），Rust 是**按 crate 名 + 大版本号**区分的，`0.9` 和 `0.10` 视为两个不同的库。

## 为什么 RustCrypto 生态特别容易踩

RustCrypto 是一组密码学 crate（`sha2`、`hmac`、`pbkdf2`、`aes`、`digest` 等），它们之间**互相依赖、且版本强绑定**。`pbkdf2 0.12` 内部依赖的是 `hmac 0.12`，而 `hmac 0.12` 又依赖 `digest 0.10`（`sha2 0.10` 也基于 `digest 0.10`）。它们通过 `digest::Digest` / `Mac` 等 **trait** 对接，而**不同大版本的 trait 是不兼容的**——`sha2 0.9` 实现的 `Digest` 和 `sha2 0.10` 实现的 `Digest` 是两个不同的 trait，不能互换。

## 于是会产生什么后果

假设你的 `Cargo.toml` 里同时有：

```toml
pbkdf2 = "0.12"   # 内部拉 hmac 0.12 + digest 0.10 + sha2 0.10
sha2   = "0.9"    # 你自己直接依赖的旧版本
```

编译后依赖树里会**同时出现 `sha2 0.9` 和 `sha2 0.10`**。这时如果代码想把自己 `use` 的 `Sha256`（来自 0.9）传给 `pbkdf2_hmac::<Sha256>`（期望 0.10 的类型），会报类似错误：

```
expected `sha2::Sha256` (sha2 0.10), found `sha2::Sha256` (sha2 0.9)
```

**两个都叫 `Sha256`，但不是一个东西**——这就是"同一 crate 两个版本并存"最典型的症状，报错信息还很迷惑，新手容易卡住。

## 为什么叫"常见坑"

因为：

1. **不报错**：两个版本并存本身合法，`cargo build` 不会提醒你；
2. **体积/性能浪费**：两份 `sha2` 都编进二进制，代码膨胀，编译变慢；
3. **API 错配**：类型/trait 对不上时，报错信息指向同名不同类型，排查费劲；
4. **只在对接时才暴露**：单独用 `sha2 0.9` 没事，一旦和 `pbkdf2 0.12` 握手就炸。

## 怎么确认和解决

**查看依赖树**：

```bash
cargo tree -d          # 只显示重复（duplicate）的依赖
cargo tree -i sha2     # 反查谁依赖了 sha2
```

如果输出里 `sha2` 出现两个版本，就是命中了。

**解决**：把项目里自己直接写的 `sha2` / `hmac` 等**统一到 `pbkdf2 0.12` 要求的版本**（`sha2 0.10`、`hmac 0.12`），或者干脆不直接依赖，让 `pbkdf2` 传递引入。改完再跑一次 `cargo tree -d`，确认没有重复。

## 回到这条建议

因为你的修复引入了 `pbkdf2 = "0.12"`，而项目里**可能本来就依赖着 `sha2` 或 `hmac` 的旧版本**（比如别处做 SHA-256 哈希、HMAC 签名时引的）。这两者如果版本对不上，就会踩上面的坑。

所以建议：

- 在报告里**记一句**"已确认 `cargo tree -d` 无重复的 sha2/hmac"；
- 或者在 **CI 里加一步** `cargo tree -d` 检查，重复就失败。

这样这次修复不仅当下正确，还防止以后有人再引入旧版本时悄悄埋雷。

**一句话**：让你确认项目别同时装两份 `sha2`/`hmac`，否则类型对不上、编译报错或二进制膨胀，而 `cargo tree -d` 就是查这个的工具。