//! Content context: articles, article categories, regions, shipping options, quotation.
use axum::extract::{Path, Query, State};
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::util::{cents_to_string, parse_i64_param, PageParams};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct RegionQuery {
    #[serde(rename = "parent")]
    parent: Option<String>,
    #[serde(rename = "type")]
    region_type: Option<String>,
}

/// GET /api/v1/regions?parent=&type=
pub async fn regions(
    State(state): State<AppState>,
    Query(q): Query<RegionQuery>,
) -> Result<Json<Value>, AppError> {
    let parent = match q.parent.as_deref() {
        None | Some("") => 0,
        Some(s) => s
            .parse::<i64>()
            .map_err(|_| AppError::Validation("parent must be an integer".to_string()))?,
    };
    let region_type = parse_i64_param(q.region_type.as_deref(), "type")?;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match region_type {
            Some(t) => (
                "SELECT region_id, region_name, region_type FROM ecs_region WHERE parent_id = ?1 AND region_type = ?2 ORDER BY region_id".to_string(),
                vec![Box::new(parent), Box::new(t)],
            ),
            None => (
                "SELECT region_id, region_name, region_type FROM ecs_region WHERE parent_id = ?1 ORDER BY region_id".to_string(),
                vec![Box::new(parent)],
            ),
        };
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "type": r.get::<_, i64>(2)?,
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

/// GET /api/v1/articles/{id}
pub async fn article_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let article_id = parse_id(&id)?;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT a.article_id, a.title, a.author, a.article_desc, a.content, a.keywords, a.cat_id, c.cat_name
                 FROM ecs_article a LEFT JOIN ecs_article_cat c ON c.cat_id = a.cat_id
                 WHERE a.article_id = ?1 AND a.is_open = 1",
                [article_id],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "title": r.get::<_, String>(1)?,
                        "author": r.get::<_, String>(2)?,
                        "desc": r.get::<_, String>(3)?,
                        "content": r.get::<_, String>(4)?,
                        "keywords": r.get::<_, String>(5)?,
                        "category_id": r.get::<_, i64>(6)?,
                        "category_name": r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    }))
                },
            )
            .optional()
            .map_err(db_err)?;
        row.ok_or_else(|| AppError::NotFound("article not found".to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/article-categories/{id}/articles
pub async fn article_category_articles(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<PageParams>,
) -> Result<Json<Value>, AppError> {
    let cat_id = parse_id(&id)?;
    let (page_no, page_size, offset) = page.resolve();
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_article_cat WHERE cat_id = ?1 AND is_show = 1",
                [cat_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists == 0 {
            return Err(AppError::NotFound("article category not found".to_string()));
        }
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_article WHERE cat_id = ?1 AND is_open = 1",
                [cat_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT article_id, title, article_desc, author FROM ecs_article
                 WHERE cat_id = ?1 AND is_open = 1 ORDER BY article_id DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([cat_id, page_size, offset], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "desc": r.get::<_, String>(2)?,
                    "author": r.get::<_, String>(3)?,
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

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ShippingOptionsQuery {
    country_id: Option<i64>,
    province_id: Option<i64>,
    city_id: Option<i64>,
    district_id: Option<i64>,
}

/// GET /api/v1/shipping-options?country_id=1&province_id=2&city_id=52&district_id=500
/// Public endpoint mirroring PHP myship.php. Region hierarchy is validated level by level.
pub async fn shipping_options(
    State(state): State<AppState>,
    Query(q): Query<ShippingOptionsQuery>,
) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut parents: Vec<(Option<i64>, Option<i64>, Option<i64>)> = Vec::new();
        for (level, id) in [
            ("province", q.province_id),
            ("city", q.city_id),
            ("district", q.district_id),
        ] {
            if let Some(rid) = id {
                let parent_id: Option<i64> = conn
                    .query_row(
                        "SELECT parent_id FROM ecs_region WHERE region_id = ?1",
                        [rid],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(db_err)?;
                let Some(parent_id) = parent_id else {
                    return Err(AppError::Validation(format!("{level} region not found")));
                };
                parents.push((Some(parent_id), Some(rid), None));
            }
        }
        // Validate child belongs to parent chain: province->country, city->province, district->city.
        if let Some(pid) = q.province_id {
            let parent: i64 = conn
                .query_row("SELECT parent_id FROM ecs_region WHERE region_id = ?1", [pid], |r| r.get(0))
                .optional()
                .map_err(db_err)?
                .ok_or_else(|| AppError::Validation("province region not found".to_string()))?;
            if let Some(cid) = q.country_id {
                if parent != cid {
                    return Err(AppError::Validation("province does not belong to country".to_string()));
                }
            }
        }
        if let (Some(p), Some(c)) = (q.province_id, q.city_id) {
            let parent: i64 = conn
                .query_row("SELECT parent_id FROM ecs_region WHERE region_id = ?1", [c], |r| r.get(0))
                .optional()
                .map_err(db_err)?
                .ok_or_else(|| AppError::Validation("city region not found".to_string()))?;
            if parent != p {
                return Err(AppError::Validation("city does not belong to province".to_string()));
            }
        }
        if let (Some(c), Some(d)) = (q.city_id, q.district_id) {
            let parent: i64 = conn
                .query_row("SELECT parent_id FROM ecs_region WHERE region_id = ?1", [d], |r| r.get(0))
                .optional()
                .map_err(db_err)?
                .ok_or_else(|| AppError::Validation("district region not found".to_string()))?;
            if parent != c {
                return Err(AppError::Validation("district does not belong to city".to_string()));
            }
        }
        let _ = parents;
        let mut stmt = conn
            .prepare("SELECT shipping_id, shipping_name, shipping_fee FROM ecs_shipping WHERE enabled = 1 ORDER BY shipping_id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                let fee: f64 = r.get(2)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "fee": cents_to_string((fee * 100.0).round() as i64),
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({
            "selected": {"country_id": q.country_id, "province_id": q.province_id, "city_id": q.city_id, "district_id": q.district_id},
            "shipping_options": items,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct QuotationQuery {
    category_id: Option<i64>,
    brand_id: Option<i64>,
    q: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

/// GET /api/v1/quotation?category_id=&brand_id=&q=&page=&page_size=
pub async fn quotation(
    State(state): State<AppState>,
    Query(q): Query<QuotationQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: q.page, page_size: q.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let db = state.db.clone();
    let category_id = q.category_id;
    let brand_id = q.brand_id;
    let keyword = q.q.clone().unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut where_clause = "WHERE g.is_on_sale = 1 AND g.is_delete = 0".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(cid) = category_id {
            where_clause.push_str(" AND g.cat_id = ?");
            params.push(Box::new(cid));
        }
        if let Some(bid) = brand_id {
            where_clause.push_str(" AND g.brand_id = ?");
            params.push(Box::new(bid));
        }
        if !keyword.is_empty() {
            where_clause.push_str(" AND g.goods_name LIKE ? ESCAPE '\\'");
            let escaped = format!("%{}%", keyword.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            params.push(Box::new(escaped));
        }
        let total: i64 = {
            let sql = format!("SELECT COUNT(*) FROM ecs_goods g {where_clause}");
            let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
            conn.query_row(&sql, params_ref.as_slice(), |r| r.get(0))
                .map_err(db_err)?
        };
        let sql = format!(
            "SELECT g.goods_id, g.goods_name, g.cat_id, c.cat_name, g.shop_price, g.goods_number
             FROM ecs_goods g LEFT JOIN ecs_category c ON c.cat_id = g.cat_id
             {where_clause} ORDER BY g.goods_id LIMIT ? OFFSET ?"
        );
        params.push(Box::new(page_size));
        params.push(Box::new(offset));
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let mut items: Vec<Value> = Vec::new();
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                let price: f64 = r.get(4)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    cents_to_string((price * 100.0).round() as i64),
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(db_err)?;
        let rows: Vec<(i64, String, i64, String, String, i64)> =
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        for (goods_id, name, cat_id, cat_name, price, stock) in rows {
            let skus: Vec<Value> = {
                let mut pstmt = conn
                    .prepare("SELECT product_id, goods_attr, product_sn, product_number FROM ecs_products WHERE goods_id = ?1 ORDER BY product_id")
                    .map_err(db_err)?;
                let prows = pstmt
                    .query_map([goods_id], |r| {
                        Ok(json!({
                            "product_id": r.get::<_, i64>(0)?,
                            "attr_signature": r.get::<_, String>(1)?,
                            "sn": r.get::<_, String>(2)?,
                            "stock": r.get::<_, i64>(3)?,
                        }))
                    })
                    .map_err(db_err)?;
                prows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
            };
            items.push(json!({
                "goods_id": goods_id,
                "name": name,
                "category_id": cat_id,
                "category_name": cat_name,
                "price": price,
                "stock": stock,
                "skus": skus,
            }));
        }
        Ok(json!({"page": page_no, "page_size": page_size, "total": total, "items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}
