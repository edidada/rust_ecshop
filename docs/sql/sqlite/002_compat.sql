-- Compat/extension schema for matrix TODO items: browsing history, PM messages, wholesale tiers.
CREATE TABLE IF NOT EXISTS ecs_browsing_history (
  user_id INTEGER NOT NULL,
  goods_id INTEGER NOT NULL,
  viewed_at INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user_id, goods_id),
  FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE,
  FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS ecs_pm (
  pm_id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  from_user_id INTEGER NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  content TEXT NOT NULL DEFAULT '',
  is_read INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  FOREIGN KEY(user_id) REFERENCES ecs_users(user_id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS ecs_wholesale (
  goods_id INTEGER NOT NULL,
  min_quantity INTEGER NOT NULL,
  price_cents INTEGER NOT NULL,
  PRIMARY KEY (goods_id, min_quantity),
  FOREIGN KEY(goods_id) REFERENCES ecs_goods(goods_id)
);
INSERT OR IGNORE INTO ecs_wholesale(goods_id, min_quantity, price_cents) VALUES(12, 10, 4590);
INSERT OR IGNORE INTO ecs_wholesale(goods_id, min_quantity, price_cents) VALUES(12, 50, 4290);
