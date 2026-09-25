# 详细设计与 C++ 编写指南

## 一个请求的路径

以 `POST /api/v1/cart/items` 为例：

1. `CartHandler::addItem` 读取 JSON，构造 `AddCartItemCommand`；未知字段可拒绝，`quantity` 必须是 1..999。
2. Auth middleware 给出 `UserId`；游客生成匿名 `CartOwner`，登录后合并购物车。
3. `AddCartItemUseCase` 调用 `GoodsRepository::findSellableForUpdate`，验证商品、规格、上下架和库存。
4. 在短事务中按 `(owner, goods_id, product_id, attr_signature)` 新增或累加 `cart`，递增 `version`。
5. 查询价格快照，组装 `CartItemResponse`，由 Handler 序列化为 JSON。

```cpp
struct AddCartItemCommand {
  std::int64_t goods_id;
  std::optional<std::int64_t> product_id;
  std::vector<std::int64_t> attribute_ids;
  std::int32_t quantity;
};

class AddCartItemUseCase {
 public:
  CartItem execute(const UserId& user, const AddCartItemCommand& command);
};
```

Handler 的返回类型不应是数据库 Row，也不要让 `nlohmann::json` 穿透到 Domain；DTO 与领域对象互相转换。

## PlaceOrder 事务

```text
begin
  锁定该用户购物车行与对应商品/货品行（固定 goods_id 顺序）
  校验地址、配送、支付方式、价格、优惠、库存
  计算总价（Money: int64 分 或 decimal string；二选一全局一致）
  INSERT order_info
  对每一行 INSERT order_goods（写商品与价格快照）
  UPDATE products/goods SET stock = stock - quantity WHERE stock >= quantity
  若任意 update 影响行数 != 1：rollback, out_of_stock
  DELETE 已结算 cart 行
  INSERT order_action 审计记录
commit
```

必须通过 `UPDATE ... WHERE stock >= ?` 保证并发安全，不能“先查库存、再无条件扣减”。按商品 ID 排序加锁可降低死锁；捕获 MySQL deadlock 后至多重试 2 次，且每次使用同一幂等键。

## Repository SQL 约定

```sql
SELECT goods_id, goods_name, shop_price, goods_number
FROM goods
WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0;
```

- `?` 只能通过驱动绑定；Repository 方法输入使用整数、字符串和明确的值对象。
- 金额列采用 `DECIMAL(12,2)`；C++ 用 `Money` 避免 `double`。
- 所有用户可见查询均显式包含 `is_delete = 0`、`is_on_sale = 1` 等可见性条件。
- 大列表用 `LIMIT ? OFFSET ?`，限制最大 page size；搜索第一版可 `LIKE` 并转义 `%`/`_`，后续迁移全文索引。

## 错误、资源与并发

- 用 `Expected<T, AppError>`（或异常只跨基础设施边界）表示可预期错误；最外层 middleware 统一转 HTTP。
- 连接池是进程级对象，连接借用对象应 RAII 自动归还；事务对象析构时未提交即 rollback。
- 捕获所有线程入口异常；日志只输出安全的用户 ID、订单号和 request ID，严禁密码、Cookie、支付签名。
- SQL connection、statement、result set 都由智能指针或明确 RAII wrapper 管理；避免裸 `new` 和全局可变单例。

## PHP 到 C++ 的迁移注意

| PHP 习惯 | C++ 服务替代 |
| --- | --- |
| `$_REQUEST` 混合 GET/POST/Cookie | 分别读取 path/query/header/JSON body |
| `intval()` 后拼 SQL | DTO 验证 + prepared statement |
| `$_SESSION` 写库 | AuthContext + Redis/DB 会话 Repository |
| `show_message()` / 页面跳转 | 明确 HTTP status + JSON error |
| 全局 `$db`, `$smarty`, `$_CFG` | 构造函数注入依赖、只读 `AppConfig` |
| MD5 密码字段 | libsodium Argon2id hash，登录时渐进迁移 |

