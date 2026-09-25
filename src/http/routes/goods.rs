//! Goods HTTP layer: routes, handlers and DTO mapping for catalog/goods URLs.
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
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

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

/// GET /api/v1/home
pub async fn home(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let categories: Vec<Value> = {
            let mut stmt = conn
                .prepare("SELECT cat_id, cat_name FROM ecs_category WHERE is_show = 1 ORDER BY sort_order, cat_id")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "name": r.get::<_,String>(1)?})))
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        let newest: Vec<Value> = goods_rows(&conn, "WHERE is_on_sale = 1 AND is_delete = 0 ORDER BY goods_id DESC LIMIT 10", [])?;
        let best: Vec<Value> = goods_rows(&conn, "WHERE is_on_sale = 1 AND is_delete = 0 AND is_best = 1 ORDER BY goods_id DESC LIMIT 10", [])?;
        let hot: Vec<Value> = goods_rows(&conn, "WHERE is_on_sale = 1 AND is_delete = 0 AND is_hot = 1 ORDER BY goods_id DESC LIMIT 10", [])?;
        let articles: Vec<Value> = {
            let mut stmt = conn
                .prepare("SELECT article_id, title FROM ecs_article WHERE is_open = 1 ORDER BY article_id DESC LIMIT 10")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| Ok(json!({"id": r.get::<_,i64>(0)?, "title": r.get::<_,String>(1)?})))
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        Ok(json!({"categories": categories, "new_goods": newest, "best_goods": best, "hot_goods": hot, "articles": articles}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

fn goods_rows(
    conn: &rusqlite::Connection,
    suffix: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Value>, AppError> {
    let sql = format!(
        "SELECT goods_id, goods_name, shop_price, market_price, goods_brief, goods_thumb FROM ecs_goods {suffix}"
    );
    let mut stmt = conn.prepare(&sql).map_err(db_err)?;
    let rows = stmt
        .query_map(params, |r| {
            let price: f64 = r.get(2)?;
            let mp: f64 = r.get(3)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "price": cents_to_string((price * 100.0).round() as i64),
                "market_price": cents_to_string((mp * 100.0).round() as i64),
                "brief": r.get::<_, String>(4)?,
                "thumb": r.get::<_, String>(5)?,
            }))
        })
        .map_err(db_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
}

/// GET /api/v1/catalog
pub async fn catalog(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let categories: Vec<Value> = {
            let mut stmt = conn
                .prepare(
                    "SELECT c.cat_id, c.cat_name, c.parent_id, c.sort_order,
                        (SELECT COUNT(*) FROM ecs_goods g WHERE g.cat_id = c.cat_id AND g.is_on_sale = 1 AND g.is_delete = 0 AND g.is_alone_sale = 1) AS goods_count
                     FROM ecs_category c WHERE c.is_show = 1 ORDER BY c.parent_id, c.sort_order, c.cat_id",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "parent_id": r.get::<_, i64>(2)?,
                        "sort_order": r.get::<_, i64>(3)?,
                        "goods_count": r.get::<_, i64>(4)?,
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        let brands: Vec<Value> = {
            let mut stmt = conn
                .prepare(
                    "SELECT b.brand_id, b.brand_name, b.brand_logo, b.site_url,
                        (SELECT COUNT(*) FROM ecs_goods g WHERE g.brand_id = b.brand_id AND g.is_on_sale = 1 AND g.is_delete = 0) AS goods_count
                     FROM ecs_brand b WHERE b.is_show = 1 ORDER BY b.sort_order, b.brand_id",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "logo": r.get::<_, String>(2)?,
                        "site_url": r.get::<_, String>(3)?,
                        "goods_count": r.get::<_, i64>(4)?,
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        Ok(json!({"categories": categories, "brands": brands}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/goods/{id}
pub async fn goods_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let goods_id = parse_id(&id)?;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let goods = conn
            .query_row(
                "SELECT goods_id, goods_name, goods_sn, brand_id, goods_number, market_price, shop_price, goods_brief, goods_desc, goods_img, goods_thumb, is_on_sale, is_delete
                 FROM ecs_goods WHERE goods_id = ?1",
                [goods_id],
                |r| {
                    let is_on_sale: i64 = r.get(11)?;
                    let is_delete: i64 = r.get(12)?;
                    if is_on_sale != 1 || is_delete != 0 {
                        return Ok(None);
                    }
                    let shop_price: f64 = r.get(6)?;
                    let market_price: f64 = r.get(5)?;
                    Ok(Some(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "sn": r.get::<_, String>(2)?,
                        "brand_id": r.get::<_, i64>(3)?,
                        "stock": r.get::<_, i64>(4)?,
                        "market_price": cents_to_string((market_price * 100.0).round() as i64),
                        "price": cents_to_string((shop_price * 100.0).round() as i64),
                        "brief": r.get::<_, String>(7)?,
                        "desc": r.get::<_, String>(8)?,
                        "img": r.get::<_, String>(9)?,
                        "thumb": r.get::<_, String>(10)?,
                    })))
                },
            )
            .optional()
            .map_err(db_err)?;
        let Some(goods) = goods else {
            return Err(AppError::NotFound("goods not found".to_string()));
        };
        let mut goods = goods.expect("matched optional row");
        let gallery: Vec<Value> = {
            let mut stmt = conn
                .prepare("SELECT img_id, img_url, img_desc, thumb_url, img_original FROM ecs_goods_gallery WHERE goods_id = ?1 ORDER BY img_id")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([goods_id], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "url": r.get::<_, String>(1)?,
                        "desc": r.get::<_, String>(2)?,
                        "thumb": r.get::<_, String>(3)?,
                        "original": r.get::<_, String>(4)?,
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        let attrs: Vec<Value> = {
            let mut stmt = conn
                .prepare("SELECT goods_attr_id, attr_id, attr_value, attr_price FROM ecs_goods_attr WHERE goods_id = ?1")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([goods_id], |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "attr_id": r.get::<_, i64>(1)?,
                        "value": r.get::<_, String>(2)?,
                        "price": r.get::<_, String>(3)?,
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        goods["gallery"] = Value::Array(gallery);
        goods["attributes"] = Value::Array(attrs);
        Ok(goods)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result).into_response())
}

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation(
            "goods id must be a positive integer".to_string(),
        )),
    }
}

/// GET /api/v1/goods (search + list)
pub async fn goods_list(
    State(state): State<AppState>,
    Query(q): Query<GoodsListQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: q.page, page_size: q.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let db = state.db.clone();
    let category_id = q.category_id;
    let brand_id = q.brand_id;
    let keyword = q.q.clone().or_else(|| q.keywords.clone()).unwrap_or_default();
    let sort = q.sort.clone().unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(cid) = category_id {
            where_clause.push_str(" AND cat_id = ?");
            params.push(Box::new(cid));
        }
        if let Some(bid) = brand_id {
            where_clause.push_str(" AND brand_id = ?");
            params.push(Box::new(bid));
        }
        if !keyword.is_empty() {
            where_clause.push_str(" AND (goods_name LIKE ? ESCAPE '\\' OR keywords LIKE ? ESCAPE '\\')");
            let escaped = format!("%{}%", escape_like(&keyword));
            params.push(Box::new(escaped.clone()));
            params.push(Box::new(escaped));
        }
        let order_by = match sort.as_str() {
            "price_asc" => "ORDER BY shop_price ASC",
            "price_desc" => "ORDER BY shop_price DESC",
            "newest" => "ORDER BY goods_id DESC",
            "sales" => "ORDER BY click_count DESC",
            _ => "ORDER BY sort_order, goods_id DESC",
        };
        let total: i64 = {
            let sql = format!("SELECT COUNT(*) FROM ecs_goods {where_clause}");
            let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
            conn.query_row(&sql, params_ref.as_slice(), |r| r.get(0))
                .map_err(db_err)?
        };
        let sql = format!(
            "SELECT goods_id, goods_name, shop_price, market_price, goods_brief, goods_thumb FROM ecs_goods {where_clause} {order_by} LIMIT ? OFFSET ?"
        );
        params.push(Box::new(page_size));
        params.push(Box::new(offset));
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                let price: f64 = r.get(2)?;
                let mp: f64 = r.get(3)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
                    "market_price": cents_to_string((mp * 100.0).round() as i64),
                    "brief": r.get::<_, String>(4)?,
                    "thumb": r.get::<_, String>(5)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"page": page_no, "page_size": page_size, "total": total, "items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
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

/// GET /api/v1/brands, GET /api/v1/brands/{id}/goods
pub async fn brands(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare("SELECT brand_id, brand_name, brand_logo, site_url FROM ecs_brand WHERE is_show = 1 ORDER BY sort_order, brand_id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "logo": r.get::<_, String>(2)?,
                    "site_url": r.get::<_, String>(3)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

pub async fn brand_goods(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<GoodsListQuery>,
) -> Result<Json<Value>, AppError> {
    let brand_id = parse_id(&id)?;
    let page = PageParams { page: q.page, page_size: q.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_goods WHERE brand_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [brand_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        let items = goods_rows_paged(&conn, brand_id, page_size, offset)?;
        Ok(json!({"page": page_no, "page_size": page_size, "total": total, "items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

fn goods_rows_paged(
    conn: &rusqlite::Connection,
    brand_id: i64,
    page_size: i64,
    offset: i64,
) -> Result<Vec<Value>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT goods_id, goods_name, shop_price, market_price, goods_brief, goods_thumb
             FROM ecs_goods WHERE brand_id = ?1 AND is_on_sale = 1 AND is_delete = 0
             ORDER BY sort_order, goods_id DESC LIMIT ?2 OFFSET ?3",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([brand_id, page_size, offset], |r| {
            let price: f64 = r.get(2)?;
            let mp: f64 = r.get(3)?;
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "price": cents_to_string((price * 100.0).round() as i64),
                "market_price": cents_to_string((mp * 100.0).round() as i64),
                "brief": r.get::<_, String>(4)?,
                "thumb": r.get::<_, String>(5)?,
            }))
        })
        .map_err(db_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
}

/// GET /api/v1/categories/{id}/goods
pub async fn category_goods(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<GoodsListQuery>,
) -> Result<Json<Value>, AppError> {
    let category_id = parse_id(&id)?;
    let page = PageParams { page: q.page, page_size: q.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        // Category must exist and be visible; otherwise 404 to avoid leaking.
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_category WHERE cat_id = ?1 AND is_show = 1",
                [category_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists == 0 {
            return Err(AppError::NotFound("category not found".to_string()));
        }
        // Collect descendant category ids (the schema has no closure table; walk one level chain).
        let mut cat_ids = vec![category_id];
        let mut frontier = vec![category_id];
        loop {
            let mut next_ids: Vec<i64> = Vec::new();
            {
                let mut stmt = conn
                    .prepare("SELECT cat_id FROM ecs_category WHERE parent_id = ?1 AND is_show = 1")
                    .map_err(db_err)?;
                for pid in &frontier {
                    let rows = stmt
                        .query_map([pid], |r| r.get::<_, i64>(0))
                        .map_err(db_err)?;
                    for row in rows {
                        let cid = row.map_err(db_err)?;
                        if !cat_ids.contains(&cid) {
                            cat_ids.push(cid);
                            next_ids.push(cid);
                        }
                    }
                }
            }
            if next_ids.is_empty() {
                break;
            }
            frontier = next_ids;
        }
        let placeholders = cat_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let params: Vec<Box<dyn rusqlite::ToSql>> =
            cat_ids.into_iter().map(|c| Box::new(c) as Box<dyn rusqlite::ToSql>).collect();
        let total: i64 = {
            let sql = format!(
                "SELECT COUNT(*) FROM ecs_goods WHERE is_on_sale = 1 AND is_delete = 0 AND cat_id IN ({placeholders})"
            );
            let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
            conn.query_row(&sql, params_ref.as_slice(), |r| r.get(0))
                .map_err(db_err)?
        };
        let sql = format!(
            "SELECT goods_id, goods_name, shop_price, market_price, goods_brief, goods_thumb FROM ecs_goods
             WHERE is_on_sale = 1 AND is_delete = 0 AND cat_id IN ({placeholders})
             ORDER BY sort_order, goods_id DESC LIMIT ? OFFSET ?"
        );
        let mut params = params;
        params.push(Box::new(page_size));
        params.push(Box::new(offset));
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                let price: f64 = r.get(2)?;
                let mp: f64 = r.get(3)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
                    "market_price": cents_to_string((mp * 100.0).round() as i64),
                    "brief": r.get::<_, String>(4)?,
                    "thumb": r.get::<_, String>(5)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"page": page_no, "page_size": page_size, "total": total, "items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/compare?goods_ids=12,14
pub async fn compare(
    State(state): State<AppState>,
    Query(q): Query<GoodsListQuery>,
) -> Result<Json<Value>, AppError> {
    let raw = q.goods_ids.ok_or_else(|| {
        AppError::Validation("goods_ids is required".to_string())
    })?;
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
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut items = Vec::new();
        for goods_id in &ids {
            let row = conn
                .query_row(
                    "SELECT goods_id, goods_name, shop_price, market_price, goods_brief, goods_number
                     FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                    [goods_id],
                    |r| {
                        let price: f64 = r.get(2)?;
                        let market: f64 = r.get(3)?;
                        Ok(json!({
                            "id": r.get::<_, i64>(0)?,
                            "name": r.get::<_, String>(1)?,
                            "price": cents_to_string((price * 100.0).round() as i64),
                            "market_price": cents_to_string((market * 100.0).round() as i64),
                            "brief": r.get::<_, String>(4)?,
                            "stock": r.get::<_, i64>(5)?,
                        }))
                    },
                )
                .optional()
                .map_err(db_err)?;
            match row {
                Some(v) => items.push(v),
                // Do not reveal invisible goods in comparison results.
                None => return Err(AppError::NotFound(format!("goods {goods_id} not found"))),
            }
        }
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
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
    let db = state.db.clone();
    let quantity = body.quantity;
    let product_id = body.product_id;
    let attribute_ids = body.attribute_ids.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT goods_id, shop_price, goods_number FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [goods_id],
                |r| {
                    let shop_price: f64 = r.get(1)?;
                    Ok((r.get::<_, i64>(0)?, (shop_price * 100.0).round() as i64, r.get::<_, i64>(2)?))
                },
            )
            .optional()
            .map_err(db_err)?;
        let Some((gid, base_price, mut stock)) = row else {
            return Err(AppError::NotFound("goods not found".to_string()));
        };
        let _ = gid;
        // Attribute price deltas are validated against the goods' own attributes.
        let mut unit_price = base_price;
        for attr_id in &attribute_ids {
            let attr_price: Option<String> = conn
                .query_row(
                    "SELECT attr_price FROM ecs_goods_attr WHERE goods_attr_id = ?1 AND goods_id = ?2",
                    [attr_id, &goods_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?;
            match attr_price {
                Some(price_str) => {
                    let delta_cents = crate::shared::util::parse_money_cents(&price_str, "attr_price")?;
                    unit_price += delta_cents;
                }
                None => return Err(AppError::Validation(format!("attribute {attr_id} does not belong to goods {goods_id}"))),
            }
        }
        // SKU product price overrides base price when provided.
        if let Some(pid) = product_id {
            let product_row: Option<i64> = conn
                .query_row(
                    "SELECT product_number FROM ecs_products WHERE product_id = ?1 AND goods_id = ?2",
                    [pid, goods_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?;
            match product_row {
                Some(pn) => stock = pn,
                None => return Err(AppError::Validation(format!("product {pid} does not belong to goods {goods_id}"))),
            }
        }
        let stock_available = stock.max(0);
        let total = unit_price
            .checked_mul(quantity)
            .ok_or_else(|| AppError::Validation("total is out of range".to_string()))?;
        Ok(json!({
            "goods_id": goods_id,
            "quantity": quantity,
            "unit_price": cents_to_string(unit_price),
            "total": cents_to_string(total),
            "currency": "CNY",
            "stock_available": stock_available,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/goods/{id}/gallery
pub async fn goods_gallery(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let goods_id = parse_id(&id)?;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let goods_name: Option<String> = conn
            .query_row(
                "SELECT goods_name FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [goods_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let Some(goods_name) = goods_name else {
            return Err(AppError::NotFound("goods not found".to_string()));
        };
        let mut stmt = conn
            .prepare("SELECT img_id, img_url, img_desc, thumb_url, img_original FROM ecs_goods_gallery WHERE goods_id = ?1 ORDER BY img_id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([goods_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "url": r.get::<_, String>(1)?,
                    "desc": r.get::<_, String>(2)?,
                    "thumb": r.get::<_, String>(3)?,
                    "original": r.get::<_, String>(4)?,
                }))
            })
            .map_err(db_err)?;
        let images: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        if images.is_empty() {
            return Err(AppError::NotFound("goods gallery is empty".to_string()));
        }
        Ok(json!({"goods_id": goods_id, "name": goods_name, "images": images}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/goods/{id}/comments (public list)
pub async fn goods_comments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let goods_id = parse_id(&id)?;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [goods_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists == 0 {
            return Err(AppError::NotFound("goods not found".to_string()));
        }
        let mut stmt = conn
            .prepare(
                "SELECT comment_id, user_name, content, add_time FROM ecs_comment
                 WHERE id_value = ?1 AND comment_type = 0 AND status = 1 ORDER BY comment_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([goods_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "user_name": r.get::<_, String>(1)?,
                    "content": r.get::<_, String>(2)?,
                    "created_at": r.get::<_, i64>(3)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/exchange-goods/{id} detail lives in marketing module; this handles 404 fallback
pub fn not_found_response() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"code": "not_found", "message": "not found"}))).into_response()
}
