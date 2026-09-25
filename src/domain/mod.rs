//! Domain layer: entities, value objects, state machines, repository traits.
//! Must not depend on axum, JSON, or database drivers.

use crate::shared::error::AppError;

/// Order state machine, mapped from the PHP `order_status`/`shipping_status`/`pay_status` columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderState {
    PendingPayment,
    Paid,
    Shipped,
    Received,
    Refunding,
    Refunded,
    Cancelled,
}

/// Money is an integer number of cents. Never use floating point for money.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Money {
    cents: i64,
}

impl Money {
    pub fn from_cents(cents: i64) -> Self {
        Self { cents }
    }

    pub fn cents(&self) -> i64 {
        self.cents
    }

    pub fn to_decimal_string(self) -> String {
        format!("{:.2}", self.cents as f64 / 100.0)
    }
}

/// Repository trait for goods. Implementations live in `infrastructure`.
pub trait GoodsRepository: Send + Sync {
    fn find_sellable_by_id(&self, id: i64) -> Result<Option<Goods>, AppError>;
}

/// Pure business data for a goods item (from the `goods` table).
#[derive(Debug, Clone)]
pub struct Goods {
    pub id: i64,
    pub name: String,
    pub price_cents: i64,
    pub stock: i32,
    pub is_on_sale: bool,
}
