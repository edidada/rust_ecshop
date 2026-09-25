# API 错误响应工厂

`ecshop::api::ApiError` 集中创建 JSON 错误响应，使 HTTP status、机器可读错误码和面向客户端的消息保持一致。

## `[[nodiscard]]`

错误工厂返回 `ApiResponse`，忽略返回值通常意味着本应返回给客户端的错误响应被遗漏。因此工厂函数使用 `[[nodiscard]]`，让编译器对这类调用给出警告：

```cpp
ApiError::notFound("goods not found"); // 应改为 return，编译器会警告
```

## 409 冲突

`conflict(code, message)` 是通用的 HTTP 409 工厂。`code` 用于客户端分支处理，`message` 用于展示或诊断。

```cpp
return ApiError::conflict("version_conflict", "resource version is stale");
return ApiError::conflict("business_rule_conflict", "order cannot be cancelled");
```

对频繁出现的领域错误，提供语义化快捷方法以避免散落的字符串字面量：

```cpp
return ApiError::outOfStock("insufficient stock or unavailable goods");
```

快捷方法必须委托给通用方法；新 409 场景应优先直接使用 `conflict`，当其成为稳定、高频领域概念后再添加命名快捷方法。

## `std::string_view`

`conflict` 和 `outOfStock` 接受 `std::string_view`，可零拷贝接收字符串字面量或 `std::string`。工厂在返回前将它们复制到 JSON，因此调用方传入的对象只需要在函数调用期间有效。
