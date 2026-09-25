//! Goods HTTP layer: routes, handlers and DTO mapping for catalog/goods URLs.
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_i64, col_str, col_opt_str, col_cents, col_f64, q, q1, e};
use crate::shared::util::{cents_to_string, PageParams};

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct GoodsListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    category_id: Option<i64>,
    brand_id: Option<i64>,
    q: Option<String>,
    sort: Option<String>,
    keywords: Option<String>,
    goods_ids: Option<String>,
}

const GOODS_LIST_COLS: &str = "goods_id AS goods_id, goods_name AS goods_name, shop_price AS shop_price, market_price AS market_price, goods_brief AS goods_brief, goods_thumb AS goods_thumb";

fn goods_item(row: &Value) -> Value {
    json!({
        "id": col_i64(row, "goods_id"),
        "name": col_str(row, "goods_name"),
        "price": cents_to_string(col_cents(row, "shop_price")),
        "market_price": cents_to_string(col_cents(row, "market_price")),
        "brief": col_str(row, "goods_brief"),
        "thumb": col_str(row, "goods_thumb"),
    })
}

/// GET /api/v1/home
pub async fn home(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let rb = state.rb;
    let categories = q(
        rb,
        "SELECT cat_id AS cat_id, cat_name AS cat_name FROM ecs_category WHERE is_show = 1 ORDER BY sort_order, cat_id",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| json!({"id": col_i64(&r, "cat_id"), "name": col_str(&r, "cat_name")}))
    .collect::<Vec<_>>();
    let newest = q(
        rb,
        &format!("SELECT {GOODS_LIST_COLS} FROM ecs_goods WHERE is_on_sale = 1 AND is_delete = 0 ORDER BY goods_id DESC LIMIT 10"),
        vec![],
    )
    .await?;
    let best = q(
        rb,
        &format!("SELECT {GOODS_LIST_COLS} FROM ecs_goods WHERE is_on_sale = 1 AND is_delete = 0 AND is_best = 1 ORDER BY goods_id DESC LIMIT 10"),
        vec![],
    )
    .await?;
    let hot = q(
        rb,
        &format!("SELECT {GOODS_LIST_COLS} FROM ecs_goods WHERE is_on_sale = 1 AND is_delete = 0 AND is_hot = 1 ORDER BY goods_id DESC LIMIT 10"),
        vec![],
    )
    .await?;
    let articles = q(
        rb,
        "SELECT article_id AS article_id, title AS title FROM ecs_article WHERE is_open = 1 ORDER BY article_id DESC LIMIT 10",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| json!({"id": col_i64(&r, "article_id"), "title": col_str(&r, "title")}))
    .collect::<Vec<_>>();
    Ok(Json(json!({
        "categories": categories,
        "new_goods": newest.iter().map(goods_item).collect::<Vec<_>>(),
        "best_goods": best.iter().map(goods_item).collect::<Vec<_>>(),
        "hot_goods": hot.iter().map(goods_item).collect::<Vec<_>>(),
        "articles": articles,
    })))
}

/// GET /api/v1/catalog
pub async fn catalog(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let rb = state.rb;
    let categories = q(
        rb,
        "SELECT c.cat_id AS cat_id, c.cat_name AS cat_name, c.parent_id AS parent_id, c.sort_order AS sort_order,
            (SELECT COUNT(*) FROM ecs_goods g WHERE g.cat_id = c.cat_id AND g.is_on_sale = 1 AND g.is_delete = 0 AND g.is_alone_sale = 1) AS goods_count
         FROM ecs_category c WHERE c.is_show = 1 ORDER BY c.parent_id, c.sort_order, c.cat_id",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "cat_id"),
            "name": col_str(&r, "cat_name"),
            "parent_id": col_i64(&r, "parent_id"),
            "sort_order": col_i64(&r, "sort_order"),
            "goods_count": col_i64(&r, "goods_count"),
        })
    })
    .collect::<Vec<_>>();
    let brands = q(
        rb,
        "SELECT b.brand_id AS brand_id, b.brand_name AS brand_name, b.brand_logo AS brand_logo, b.site_url AS site_url,
            (SELECT COUNT(*) FROM ecs_goods g WHERE g.brand_id = b.brand_id AND g.is_on_sale = 1 AND g.is_delete = 0) AS goods_count
         FROM ecs_brand b WHERE b.is_show = 1 ORDER BY b.sort_order, b.brand_id",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "brand_id"),
            "name": col_str(&r, "brand_name"),
            "logo": col_str(&r, "brand_logo"),
            "site_url": col_str(&r, "site_url"),
            "goods_count": col_i64(&r, "goods_count"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"categories": categories, "brands": brands})))
}

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation(
            "id must be a positive integer".to_string(),
        )),
    }
}

/// GET /api/v1/goods/{id}
pub async fn goods_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let goods_id = parse_id(&id)?;
    let rb = state.rb;
    let row = q1(
        rb,
        "SELECT goods_id AS goods_id, goods_name AS goods_name, goods_sn AS goods_sn, brand_id AS brand_id, goods_number AS goods_number,
                market_price AS market_price, shop_price AS shop_price, goods_brief AS goods_brief, goods_desc AS goods_desc,
                goods_img AS goods_img, goods_thumb AS goods_thumb, is_on_sale AS is_on_sale, is_delete AS is_delete
         FROM ecs_goods WHERE goods_id = ?",
        vec![json!(goods_id)],
    )
    .await?
    .filter(|r| col_i64(r, "is_on_sale") == 1 && col_i64(r, "is_delete") == 0)
    .ok_or_else(|| AppError::NotFound("goods not found".to_string()))?;
    let gallery = q(
        rb,
        "SELECT img_id AS img_id, img_url AS img_url, img_desc AS img_desc, thumb_url AS thumb_url, img_original AS img_original
         FROM ecs_goods_gallery WHERE goods_id = ? ORDER BY img_id",
        vec![json!(goods_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "img_id"),
            "url": col_str(&r, "img_url"),
            "desc": col_str(&r, "img_desc"),
            "thumb": col_str(&r, "thumb_url"),
            "original": col_str(&r, "img_original"),
        })
    })
    .collect::<Vec<_>>();
    let attrs = q(
        rb,
        "SELECT goods_attr_id AS goods_attr_id, attr_id AS attr_id, attr_value AS attr_value, attr_price AS attr_price
         FROM ecs_goods_attr WHERE goods_id = ?",
        vec![json!(goods_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "goods_attr_id"),
            "attr_id": col_i64(&r, "attr_id"),
            "value": col_str(&r, "attr_value"),
            "price": col_str(&r, "attr_price"),
        })
    })
    .collect::<Vec<_>>();
    let goods = json!({
        "id": col_i64(&row, "goods_id"),
        "name": col_str(&row, "goods_name"),
        "sn": col_str(&row, "goods_sn"),
        "brand_id": col_i64(&row, "brand_id"),
        "stock": col_i64(&row, "goods_number"),
        "market_price": cents_to_string(col_cents(&row, "market_price")),
        "price": cents_to_string(col_cents(&row, "shop_price")),
        "brief": col_str(&row, "goods_brief"),
        "desc": col_str(&row, "goods_desc"),
        "img": col_str(&row, "goods_img"),
        "thumb": col_str(&row, "goods_thumb"),
        "gallery": gallery,
        "attributes": attrs,
    });
    Ok(Json(goods).into_response())
}

/// GET /api/v1/goods (search + list)
pub async fn goods_list(
    State(state): State<AppState>,
    Query(qp): Query<GoodsListQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: qp.page, page_size: qp.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let rb = state.rb;
    let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
    let mut args: Vec<Value> = Vec::new();
    if let Some(cid) = qp.category_id {
        where_clause.push_str(" AND cat_id = ?");
        args.push(json!(cid));
    }
    if let Some(bid) = qp.brand_id {
        where_clause.push_str(" AND brand_id = ?");
        args.push(json!(bid));
    }
    let keyword = qp.q.clone().or_else(|| qp.keywords.clone()).unwrap_or_default();
    if !keyword.is_empty() {
        where_clause.push_str(" AND (goods_name LIKE ? ESCAPE '\\' OR keywords LIKE ? ESCAPE '\\')");
        let escaped = format!("%{}%", escape_like(&keyword));
        args.push(json!(escaped.clone()));
        args.push(json!(escaped));
    }
    let order_by = match qp.sort.as_deref() {
        Some("price_asc") => "ORDER BY shop_price ASC",
        Some("price_desc") => "ORDER BY shop_price DESC",
        Some("newest") => "ORDER BY goods_id DESC",
        Some("sales") => "ORDER BY click_count DESC",
        _ => "ORDER BY sort_order, goods_id DESC",
    };
    let total_row = q1(
        rb,
        &format!("SELECT COUNT(*) AS cnt FROM ecs_goods {where_clause}"),
        args.clone(),
    )
    .await?
    .unwrap_or_default();
    let total = col_i64(&total_row, "cnt");
    args.push(json!(page_size));
    args.push(json!(offset));
    let items = q(
        rb,
        &format!("SELECT {GOODS_LIST_COLS} FROM ecs_goods {where_clause} {order_by} LIMIT ? OFFSET ?"),
        args,
    )
    .await?
    .iter()
    .map(goods_item)
    .collect::<Vec<_>>();
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "items": items})))
}

fn escape_like(input: &str) -> String {
    input
        .chars()
        .flat_map(|c| {
            if c == '%' || c == '_' || c == '\\' {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}

/// GET /api/v1/brands
pub async fn brands(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT brand_id AS brand_id, brand_name AS brand_name, brand_logo AS brand_logo, site_url AS site_url
         FROM ecs_brand WHERE is_show = 1 ORDER BY sort_order, brand_id",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "brand_id"),
            "name": col_str(&r, "brand_name"),
            "logo": col_str(&r, "brand_logo"),
            "site_url": col_str(&r, "site_url"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// GET /api/v1/brands/{id}/goods
pub async fn brand_goods(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(qp): Query<GoodsListQuery>,
) -> Result<Json<Value>, AppError> {
    let brand_id = parse_id(&id)?;
    let page = PageParams { page: qp.page, page_size: qp.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let rb = state.rb;
    let total_row = q1(
        rb,
        "SELECT COUNT(*) AS cnt FROM ecs_goods WHERE brand_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(brand_id)],
    )
    .await?
    .unwrap_or_default();
    let total = col_i64(&total_row, "cnt");
    let items = q(
        rb,
        &format!(
            "SELECT {GOODS_LIST_COLS} FROM ecs_goods WHERE brand_id = ? AND is_on_sale = 1 AND is_delete = 0
             ORDER BY sort_order, goods_id DESC LIMIT ? OFFSET ?"
        ),
        vec![json!(brand_id), json!(page_size), json!(offset)],
    )
    .await?
    .iter()
    .map(goods_item)
    .collect::<Vec<_>>();
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "items": items})))
}

/// GET /api/v1/categories/{id}/goods
pub async fn category_goods(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(qp): Query<GoodsListQuery>,
) -> Result<Json<Value>, AppError> {
    let category_id = parse_id(&id)?;
    let page = PageParams { page: qp.page, page_size: qp.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let rb = state.rb;
    let exists = q1(
        rb,
        "SELECT COUNT(*) AS cnt FROM ecs_category WHERE cat_id = ? AND is_show = 1",
        vec![json!(category_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if exists == 0 {
        return Err(AppError::NotFound("category not found".to_string()));
    }
    // Collect descendant category ids (the schema has no closure table; walk the chain).
    let mut cat_ids = vec![category_id];
    let mut frontier = vec![category_id];
    loop {
        let mut next_ids: Vec<i64> = Vec::new();
        for pid in &frontier {
            for row in q(
                rb,
                "SELECT cat_id AS cat_id FROM ecs_category WHERE parent_id = ? AND is_show = 1",
                vec![json!(pid)],
            )
            .await?
            {
                let cid = col_i64(&row, "cat_id");
                if !cat_ids.contains(&cid) {
                    cat_ids.push(cid);
                    next_ids.push(cid);
                }
            }
        }
        if next_ids.is_empty() {
            break;
        }
        frontier = next_ids;
    }
    let placeholders = cat_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut args: Vec<Value> = cat_ids.iter().map(|c| json!(c)).collect();
    let total_row = q1(
        rb,
        &format!(
            "SELECT COUNT(*) AS cnt FROM ecs_goods WHERE is_on_sale = 1 AND is_delete = 0 AND cat_id IN ({placeholders})"
        ),
        args.clone(),
    )
    .await?
    .unwrap_or_default();
    let total = col_i64(&total_row, "cnt");
    args.push(json!(page_size));
    args.push(json!(offset));
    let items = q(
        rb,
        &format!(
            "SELECT {GOODS_LIST_COLS} FROM ecs_goods WHERE is_on_sale = 1 AND is_delete = 0 AND cat_id IN ({placeholders})
             ORDER BY sort_order, goods_id DESC LIMIT ? OFFSET ?"
        ),
        args,
    )
    .await?
    .iter()
    .map(goods_item)
    .collect::<Vec<_>>();
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "items": items})))
}

/// GET /api/v1/compare?goods_ids=12,14
pub async fn compare(
    State(state): State<AppState>,
    Query(qp): Query<GoodsListQuery>,
) -> Result<Json<Value>, AppError> {
    let raw = qp.goods_ids.ok_or_else(|| AppError::Validation("goods_ids is required".to_string()))?;
    let ids: Vec<i64> = raw
        .split(',')
        .map(|s| s.trim().parse::<i64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::Validation("goods_ids must be integers".to_string()))?;
    if ids.len() < 2 || ids.len() > 4 {
        return Err(AppError::Validation("goods_ids must contain 2 to 4 items".to_string()));
    }
    if ids.iter().any(|i| *i <= 0) {
        return Err(AppError::Validation("goods_ids must be positive".to_string()));
    }
    if ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len() {
        return Err(AppError::Validation("goods_ids must be unique".to_string()));
    }
    let rb = state.rb;
    let mut items = Vec::new();
    for goods_id in &ids {
        let row = q1(
            rb,
            "SELECT goods_id AS goods_id, goods_name AS goods_name, shop_price AS shop_price, market_price AS market_price,
                    goods_brief AS goods_brief, goods_number AS goods_number
             FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
            vec![json!(goods_id)],
        )
        .await?;
        let Some(row) = row else {
            // Do not reveal invisible goods in comparison results.
            return Err(AppError::NotFound(format!("goods {goods_id} not found")));
        };
        items.push(json!({
            "id": col_i64(&row, "goods_id"),
            "name": col_str(&row, "goods_name"),
            "price": cents_to_string(col_cents(&row, "shop_price")),
            "market_price": cents_to_string(col_cents(&row, "market_price")),
            "brief": col_str(&row, "goods_brief"),
            "stock": col_i64(&row, "goods_number"),
        }));
    }
    Ok(Json(json!({"items": items})))
}

/// POST /api/v1/goods/{id}/price-quote
#[derive(Deserialize)]
pub struct PriceQuoteRequest {
    quantity: i64,
    product_id: Option<i64>,
    attribute_ids: Option<Vec<i64>>,
}

pub async fn price_quote(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PriceQuoteRequest>,
) -> Result<Json<Value>, AppError> {
    let goods_id = parse_id(&id)?;
    if body.quantity < 1 || body.quantity > 999 {
        return Err(AppError::Validation("quantity must be between 1 and 999".to_string()));
    }
    let rb = state.rb;
    let row = q1(
        rb,
        "SELECT goods_id AS goods_id, shop_price AS shop_price, goods_number AS goods_number
         FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(goods_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("goods not found".to_string()))?;
    let mut unit_price = col_cents(&row, "shop_price");
    let mut stock = col_i64(&row, "goods_number");
    for attr_id in body.attribute_ids.unwrap_or_default() {
        let attr_row = q1(
            rb,
            "SELECT attr_price AS attr_price FROM ecs_goods_attr WHERE goods_attr_id = ? AND goods_id = ?",
            vec![json!(attr_id), json!(goods_id)],
        )
        .await?
        .ok_or_else(|| {
            AppError::Validation(format!("attribute {attr_id} does not belong to goods {goods_id}"))
        })?;
        unit_price += col_cents(&attr_row, "attr_price");
    }
    if let Some(pid) = body.product_id {
        let product_row = q1(
            rb,
            "SELECT product_number AS product_number FROM ecs_products WHERE product_id = ? AND goods_id = ?",
            vec![json!(pid), json!(goods_id)],
        )
        .await?
        .ok_or_else(|| {
            AppError::Validation(format!("product {pid} does not belong to goods {goods_id}"))
        })?;
        stock = col_i64(&product_row, "product_number");
    }
    let stock_available = stock.max(0);
    let total = unit_price
        .checked_mul(body.quantity)
        .ok_or_else(|| AppError::Validation("total is out of range".to_string()))?;
    Ok(Json(json!({
        "goods_id": goods_id,
        "quantity": body.quantity,
        "unit_price": cents_to_string(unit_price),
        "total": cents_to_string(total),
        "currency": "CNY",
        "stock_available": stock_available,
    })))
}

/// GET /api/v1/goods/{id}/gallery
pub async fn goods_gallery(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let goods_id = parse_id(&id)?;
    let rb = state.rb;
    let goods_name = q1(
        rb,
        "SELECT goods_name AS goods_name FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(goods_id)],
    )
    .await?
    .map(|r| col_str(&r, "goods_name"))
    .ok_or_else(|| AppError::NotFound("goods not found".to_string()))?;
    let images = q(
        rb,
        "SELECT img_id AS img_id, img_url AS img_url, img_desc AS img_desc, thumb_url AS thumb_url, img_original AS img_original
         FROM ecs_goods_gallery WHERE goods_id = ? ORDER BY img_id",
        vec![json!(goods_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "img_id"),
            "url": col_str(&r, "img_url"),
            "desc": col_str(&r, "img_desc"),
            "thumb": col_str(&r, "thumb_url"),
            "original": col_str(&r, "img_original"),
        })
    })
    .collect::<Vec<_>>();
    if images.is_empty() {
        return Err(AppError::NotFound("goods gallery is empty".to_string()));
    }
    Ok(Json(json!({"goods_id": goods_id, "name": goods_name, "images": images})))
}

/// GET /api/v1/goods/{id}/comments (public list)
pub async fn goods_comments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let goods_id = parse_id(&id)?;
    let rb = state.rb;
    let exists = q1(
        rb,
        "SELECT COUNT(*) AS cnt FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(goods_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if exists == 0 {
        return Err(AppError::NotFound("goods not found".to_string()));
    }
    let items = q(
        rb,
        "SELECT comment_id AS comment_id, user_name AS user_name, content AS content, add_time AS add_time
         FROM ecs_comment WHERE id_value = ? AND comment_type = 0 AND status = 1 ORDER BY comment_id DESC",
        vec![json!(goods_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "comment_id"),
            "user_name": col_str(&r, "user_name"),
            "content": col_str(&r, "content"),
            "created_at": col_i64(&r, "add_time"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// Shared write helper kept for symmetry with other modules.
#[allow(dead_code)]
async fn _noop_write(rb: &rbatis::rbatis::RBatis) -> Result<(), AppError> {
    e(rb, "SELECT 1", vec![]).await?;
    Ok(())
}

#[allow(dead_code)]
fn _keep_col_helpers(row: &Value) {
    let _ = col_f64(row, "x");
    let _ = col_opt_str(row, "x");
}
