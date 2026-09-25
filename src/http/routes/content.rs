//! Content context: articles, article categories, regions, shipping options, quotation.
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_i64, col_opt_str, col_str, col_cents, col_f64, q, q1};
use crate::shared::util::{cents_to_string, parse_i64_param, PageParams};

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
    Query(qq): Query<RegionQuery>,
) -> Result<Json<Value>, AppError> {
    let parent = match qq.parent.as_deref() {
        None | Some("") => 0,
        Some(s) => s
            .parse::<i64>()
            .map_err(|_| AppError::Validation("parent must be an integer".to_string()))?,
    };
    let region_type = parse_i64_param(qq.region_type.as_deref(), "type")?;
    let (sql, args): (String, Vec<Value>) = match region_type {
        Some(t) => (
            "SELECT region_id AS region_id, region_name AS region_name, region_type AS region_type
             FROM ecs_region WHERE parent_id = ? AND region_type = ? ORDER BY region_id"
                .to_string(),
            vec![json!(parent), json!(t)],
        ),
        None => (
            "SELECT region_id AS region_id, region_name AS region_name, region_type AS region_type
             FROM ecs_region WHERE parent_id = ? ORDER BY region_id"
                .to_string(),
            vec![json!(parent)],
        ),
    };
    let items = q(state.rb, &sql, args)
        .await?
        .into_iter()
        .map(|r| {
            json!({
                "id": col_i64(&r, "region_id"),
                "name": col_str(&r, "region_name"),
                "type": col_i64(&r, "region_type"),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// GET /api/v1/articles/{id}
pub async fn article_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let article_id = parse_id(&id)?;
    let row = q1(
        state.rb,
        "SELECT a.article_id AS article_id, a.title AS title, a.author AS author, a.article_desc AS article_desc,
                a.content AS content, a.keywords AS keywords, a.cat_id AS cat_id, c.cat_name AS cat_name
         FROM ecs_article a LEFT JOIN ecs_article_cat c ON c.cat_id = a.cat_id
         WHERE a.article_id = ? AND a.is_open = 1",
        vec![json!(article_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("article not found".to_string()))?;
    Ok(Json(json!({
        "id": col_i64(&row, "article_id"),
        "title": col_str(&row, "title"),
        "author": col_str(&row, "author"),
        "desc": col_str(&row, "article_desc"),
        "content": col_str(&row, "content"),
        "keywords": col_str(&row, "keywords"),
        "category_id": col_i64(&row, "cat_id"),
        "category_name": col_opt_str(&row, "cat_name").unwrap_or_default(),
    })))
}

/// GET /api/v1/article-categories/{id}/articles
pub async fn article_category_articles(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<PageParams>,
) -> Result<Json<Value>, AppError> {
    let cat_id = parse_id(&id)?;
    let (page_no, page_size, offset) = page.resolve();
    let rb = state.rb;
    let exists = q1(
        rb,
        "SELECT COUNT(*) AS cnt FROM ecs_article_cat WHERE cat_id = ? AND is_show = 1",
        vec![json!(cat_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if exists == 0 {
        return Err(AppError::NotFound("article category not found".to_string()));
    }
    let total = q1(
        rb,
        "SELECT COUNT(*) AS cnt FROM ecs_article WHERE cat_id = ? AND is_open = 1",
        vec![json!(cat_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    let items = q(
        rb,
        "SELECT article_id AS article_id, title AS title, article_desc AS article_desc, author AS author
         FROM ecs_article WHERE cat_id = ? AND is_open = 1 ORDER BY article_id DESC LIMIT ? OFFSET ?",
        vec![json!(cat_id), json!(page_size), json!(offset)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "article_id"),
            "title": col_str(&r, "title"),
            "desc": col_str(&r, "article_desc"),
            "author": col_str(&r, "author"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "items": items})))
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
    Query(qq): Query<ShippingOptionsQuery>,
) -> Result<Json<Value>, AppError> {
    let rb = state.rb;
    async fn parent_of(rb: &rbatis::rbatis::RBatis, region_id: i64) -> Result<Option<i64>, AppError> {
        Ok(q1(
            rb,
            "SELECT parent_id AS parent_id FROM ecs_region WHERE region_id = ?",
            vec![json!(region_id)],
        )
        .await?
        .map(|r| col_i64(&r, "parent_id")))
    }
    if let Some(pid) = qq.province_id {
        let parent = parent_of(rb, pid)
            .await?
            .ok_or_else(|| AppError::Validation("province region not found".to_string()))?;
        if let Some(cid) = qq.country_id {
            if parent != cid {
                return Err(AppError::Validation("province does not belong to country".to_string()));
            }
        }
    }
    if let (Some(p), Some(c)) = (qq.province_id, qq.city_id) {
        let parent = parent_of(rb, c)
            .await?
            .ok_or_else(|| AppError::Validation("city region not found".to_string()))?;
        if parent != p {
            return Err(AppError::Validation("city does not belong to province".to_string()));
        }
    }
    if let (Some(c), Some(d)) = (qq.city_id, qq.district_id) {
        let parent = parent_of(rb, d)
            .await?
            .ok_or_else(|| AppError::Validation("district region not found".to_string()))?;
        if parent != c {
            return Err(AppError::Validation("district does not belong to city".to_string()));
        }
    }
    let items = q(
        rb,
        "SELECT shipping_id AS shipping_id, shipping_name AS shipping_name, shipping_fee AS shipping_fee
         FROM ecs_shipping WHERE enabled = 1 ORDER BY shipping_id",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "shipping_id"),
            "name": col_str(&r, "shipping_name"),
            "fee": cents_to_string(col_cents(&r, "shipping_fee")),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({
        "selected": {"country_id": qq.country_id, "province_id": qq.province_id, "city_id": qq.city_id, "district_id": qq.district_id},
        "shipping_options": items,
    })))
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
    Query(qq): Query<QuotationQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: qq.page, page_size: qq.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let rb = state.rb;
    let mut where_clause = "WHERE g.is_on_sale = 1 AND g.is_delete = 0".to_string();
    let mut args: Vec<Value> = Vec::new();
    if let Some(cid) = qq.category_id {
        where_clause.push_str(" AND g.cat_id = ?");
        args.push(json!(cid));
    }
    if let Some(bid) = qq.brand_id {
        where_clause.push_str(" AND g.brand_id = ?");
        args.push(json!(bid));
    }
    let keyword = qq.q.clone().unwrap_or_default();
    if !keyword.is_empty() {
        where_clause.push_str(" AND g.goods_name LIKE ? ESCAPE '\\'");
        let escaped = format!(
            "%{}%",
            keyword.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
        );
        args.push(json!(escaped));
    }
    let total = q1(
        rb,
        &format!("SELECT COUNT(*) AS cnt FROM ecs_goods g {where_clause}"),
        args.clone(),
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    args.push(json!(page_size));
    args.push(json!(offset));
    let rows = q(
        rb,
        &format!(
            "SELECT g.goods_id AS goods_id, g.goods_name AS goods_name, g.cat_id AS cat_id, c.cat_name AS cat_name,
                    g.shop_price AS shop_price, g.goods_number AS goods_number
             FROM ecs_goods g LEFT JOIN ecs_category c ON c.cat_id = g.cat_id
             {where_clause} ORDER BY g.goods_id LIMIT ? OFFSET ?"
        ),
        args,
    )
    .await?;
    let mut items = Vec::new();
    for row in rows {
        let goods_id = col_i64(&row, "goods_id");
        let skus = q(
            rb,
            "SELECT product_id AS product_id, goods_attr AS goods_attr, product_sn AS product_sn, product_number AS product_number
             FROM ecs_products WHERE goods_id = ? ORDER BY product_id",
            vec![json!(goods_id)],
        )
        .await?
        .into_iter()
        .map(|r| {
            json!({
                "product_id": col_i64(&r, "product_id"),
                "attr_signature": col_str(&r, "goods_attr"),
                "sn": col_str(&r, "product_sn"),
                "stock": col_i64(&r, "product_number"),
            })
        })
        .collect::<Vec<_>>();
        items.push(json!({
            "goods_id": goods_id,
            "name": col_str(&row, "goods_name"),
            "category_id": col_i64(&row, "cat_id"),
            "category_name": col_opt_str(&row, "cat_name").unwrap_or_default(),
            "price": cents_to_string(col_cents(&row, "shop_price")),
            "stock": col_i64(&row, "goods_number"),
            "skus": skus,
        }));
    }
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "items": items})))
}

#[allow(dead_code)]
fn _keep(row: &Value) -> f64 {
    col_f64(row, "x")
}
