//! Widgets and XML endpoints: ads, goods widget JS, cycle-image.xml, sitemap.xml, feed.xml.
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::util::{cents_to_string, parse_i64_param, unix_now};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

fn xml_response(body: String) -> Response {
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
        .into_response()
}

/// GET /api/v1/ads/{id}
pub async fn ad_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let ad_id = id
        .parse::<i64>()
        .map_err(|_| AppError::Validation("ad id must be an integer".to_string()))?;
    let now = unix_now();
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT ad_id, media_type, ad_name, ad_link, ad_code, click_count
                 FROM ecs_ad
                 WHERE ad_id = ?1 AND enabled = 1 AND start_time <= ?2 AND end_time > ?3",
                [ad_id, now, now],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "media_type": r.get::<_, i64>(1)?,
                        "name": r.get::<_, String>(2)?,
                        "link": r.get::<_, String>(3)?,
                        "code": r.get::<_, String>(4)?,
                        "click_count": r.get::<_, i64>(5)?,
                    }))
                },
            )
            .optional()
            .map_err(db_err)?;
        row.ok_or_else(|| AppError::NotFound("ad not found".to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct AdClickRequest {
    #[serde(default)]
    referer: String,
}

/// POST /api/v1/ads/{id}/click — increments click_count and upserts adsense in one transaction.
pub async fn ad_click(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AdClickRequest>,
) -> Result<Json<Value>, AppError> {
    let ad_id = id
        .parse::<i64>()
        .map_err(|_| AppError::Validation("ad id must be an integer".to_string()))?;
    let now = unix_now();
    let db = state.db.clone();
    let referer = body.referer;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let link: Option<String> = tx
            .query_row(
                "SELECT ad_link FROM ecs_ad WHERE ad_id = ?1 AND enabled = 1 AND start_time <= ?2 AND end_time > ?3",
                [ad_id, now, now],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let Some(link) = link else {
            return Err(AppError::NotFound("ad not found".to_string()));
        };
        tx.execute(
            "UPDATE ecs_ad SET click_count = click_count + 1 WHERE ad_id = ?1",
            [ad_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT INTO ecs_adsense (from_ad, referer, clicks) VALUES (?1, ?2, 1)
             ON CONFLICT(from_ad, referer) DO UPDATE SET clicks = clicks + 1",
            rusqlite::params![ad_id, referer],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(json!({"redirect_url": link}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /cycle-image.xml — image ads (media_type=0) as bcaster XML.
pub async fn cycle_image(State(state): State<AppState>) -> Result<Response, AppError> {
    let db = state.db.clone();
    let now = unix_now();
    let body = tokio::task::spawn_blocking(move || -> Result<String, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT ad_id, ad_link, ad_code FROM ecs_ad
                 WHERE media_type = 0 AND enabled = 1 AND start_time <= ?1 AND end_time > ?2
                 ORDER BY position_id, ad_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([now, now], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(db_err)?;
        let rows: Vec<(i64, String, String)> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<bcaster>");
        for (ad_id, link, code) in rows {
            xml.push_str(&format!(
                "<item id=\"{}\" link=\"{}\">{}</item>",
                ad_id,
                escape_xml(&link),
                escape_xml(&code)
            ));
        }
        xml.push_str("</bcaster>");
        Ok(xml)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(xml_response(body))
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct WidgetQuery {
    cat_id: Option<String>,
    brand_id: Option<String>,
    goods_num: Option<String>,
    intro_type: Option<String>,
}

/// GET /goods-widget.js?cat_id=1&brand_id=1&goods_num=10&intro_type=is_new
/// Public JavaScript widget; caps goods_num at 50; sets window.cppEcshopGoodsWidget.
pub async fn goods_widget(
    State(state): State<AppState>,
    Query(q): Query<WidgetQuery>,
) -> Result<Response, AppError> {
    let cat_id = parse_i64_param(q.cat_id.as_deref(), "cat_id")?;
    let brand_id = parse_i64_param(q.brand_id.as_deref(), "brand_id")?;
    let goods_num = parse_i64_param(q.goods_num.as_deref(), "goods_num")?.unwrap_or(10);
    if goods_num < 1 || goods_num > 50 {
        return Err(AppError::Validation("goods_num must be between 1 and 50".to_string()));
    }
    let intro_type = q.intro_type.clone().unwrap_or_else(|| "is_new".to_string());
    if !matches!(
        intro_type.as_str(),
        "is_best" | "is_new" | "is_hot" | "is_promote" | "is_random"
    ) {
        return Err(AppError::Validation("intro_type must be is_best|is_new|is_hot|is_promote|is_random".to_string()));
    }
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let filter_col = match intro_type.as_str() {
            "is_best" => "is_best",
            "is_hot" => "is_hot",
            "is_promote" => "is_promote",
            _ => "",
        };
        let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        match intro_type.as_str() {
            "is_new" => where_clause.push_str(" ORDER BY goods_id DESC"),
            "is_random" => where_clause.push_str(" ORDER BY RANDOM()"),
            _ => {
                where_clause.push_str(&format!(" AND {filter_col} = 1 ORDER BY goods_id DESC"));
            }
        }
        if let Some(cid) = cat_id {
            where_clause = where_clause.replace("WHERE", &format!("WHERE cat_id = ? AND"));
            params.push(Box::new(cid));
        }
        if let Some(bid) = brand_id {
            where_clause = where_clause.replace("WHERE", &format!("WHERE brand_id = ? AND"));
            params.push(Box::new(bid));
        }
        // Rebuild cleanly instead of string surgery above.
        let mut conditions = vec!["is_on_sale = 1".to_string(), "is_delete = 0".to_string()];
        let mut clean_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(cid) = cat_id {
            conditions.push("cat_id = ?".to_string());
            clean_params.push(Box::new(cid));
        }
        if let Some(bid) = brand_id {
            conditions.push("brand_id = ?".to_string());
            clean_params.push(Box::new(bid));
        }
        let order = match intro_type.as_str() {
            "is_new" => "ORDER BY goods_id DESC".to_string(),
            "is_random" => "ORDER BY RANDOM()".to_string(),
            "is_best" => "AND is_best = 1 ORDER BY goods_id DESC".to_string(),
            "is_hot" => "AND is_hot = 1 ORDER BY goods_id DESC".to_string(),
            "is_promote" => "AND is_promote = 1 ORDER BY goods_id DESC".to_string(),
            _ => "ORDER BY goods_id DESC".to_string(),
        };
        let _ = where_clause;
        let _ = params;
        let sql = format!(
            "SELECT goods_id, goods_name, shop_price FROM ecs_goods WHERE {} {} LIMIT {}",
            conditions.join(" AND "),
            order,
            goods_num
        );
        let params_ref: Vec<&dyn rusqlite::ToSql> = clean_params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                let price: f64 = r.get(2)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"intro_type": intro_type, "goods": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    let js = format!(
        "window.cppEcshopGoodsWidget = {};",
        serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
    );
    Ok((
        [(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
        js,
    )
        .into_response())
}

/// GET /sitemap.xml — home, visible categories, article categories, up to 300 public goods/articles.
pub async fn sitemap(State(state): State<AppState>) -> Result<Response, AppError> {
    let base_url = state.config.site_base_url.clone();
    let db = state.db.clone();
    let body = tokio::task::spawn_blocking(move || -> Result<String, AppError> {
        let conn = db.blocking_lock();
        let mut urls: Vec<String> = vec![format!("{}/", base_url)];
        {
            let mut stmt = conn
                .prepare("SELECT cat_id FROM ecs_category WHERE is_show = 1 ORDER BY cat_id")
                .map_err(db_err)?;
            let ids: Vec<i64> = stmt
                .query_map([], |r| r.get(0))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            for id in ids {
                urls.push(format!("{}/#/category/{}", base_url, id));
            }
        }
        {
            let mut stmt = conn
                .prepare("SELECT cat_id FROM ecs_article_cat WHERE is_show = 1 ORDER BY cat_id")
                .map_err(db_err)?;
            let ids: Vec<i64> = stmt
                .query_map([], |r| r.get(0))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            for id in ids {
                urls.push(format!("{}/#/article-cat/{}", base_url, id));
            }
        }
        {
            let mut stmt = conn
                .prepare(
                    "SELECT goods_id FROM ecs_goods WHERE is_on_sale = 1 AND is_delete = 0 ORDER BY goods_id LIMIT 300",
                )
                .map_err(db_err)?;
            let ids: Vec<i64> = stmt
                .query_map([], |r| r.get(0))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            for id in ids {
                urls.push(format!("{}/#/goods/{}", base_url, id));
            }
        }
        {
            let mut stmt = conn
                .prepare("SELECT article_id FROM ecs_article WHERE is_open = 1 ORDER BY article_id LIMIT 300")
                .map_err(db_err)?;
            let ids: Vec<i64> = stmt
                .query_map([], |r| r.get(0))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?;
            for id in ids {
                urls.push(format!("{}/#/article/{}", base_url, id));
            }
        }
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">");
        for u in urls {
            xml.push_str(&format!("<url><loc>{}</loc></url>", escape_xml(&u)));
        }
        xml.push_str("</urlset>");
        Ok(xml)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(xml_response(body))
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct FeedQuery {
    cat: Option<String>,
    brand: Option<String>,
    #[serde(rename = "type")]
    feed_type: Option<String>,
}

/// GET /feed.xml — RSS 2.0 of latest public goods, optional cat/brand/type filters.
pub async fn feed(
    State(state): State<AppState>,
    Query(q): Query<FeedQuery>,
) -> Result<Response, AppError> {
    let base_url = state.config.site_base_url.clone();
    let cat = parse_i64_param(q.cat.as_deref(), "cat")?;
    let brand = parse_i64_param(q.brand.as_deref(), "brand")?;
    let feed_type = q.feed_type.clone().unwrap_or_default();
    let db = state.db.clone();
    let body = tokio::task::spawn_blocking(move || -> Result<String, AppError> {
        let conn = db.blocking_lock();
        let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(cid) = cat {
            where_clause.push_str(" AND cat_id = ?");
            params.push(Box::new(cid));
        }
        if let Some(bid) = brand {
            where_clause.push_str(" AND brand_id = ?");
            params.push(Box::new(bid));
        }
        // type filters map to goods_activity act_type joins.
        let act_type: Option<i64> = match feed_type.as_str() {
            "group_buy" => Some(1),
            "snatch" => Some(0),
            "auction" => Some(2),
            "exchange" => Some(5),
            "activity" => None,
            "package" => Some(4),
            other if other.starts_with("article_cat") => None,
            "" => None,
            _ => None,
        };
        let items: Vec<(String, String, String)>;
        if feed_type.starts_with("article_cat") {
            let cat_id: i64 = feed_type
                .trim_start_matches("article_cat")
                .parse()
                .unwrap_or(0);
            let mut stmt = conn
                .prepare(
                    "SELECT title, article_desc, article_id FROM ecs_article WHERE cat_id = ?1 AND is_open = 1 ORDER BY article_id DESC LIMIT 100",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([cat_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?.to_string(),
                    ))
                })
                .map_err(db_err)?;
            items = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        } else if let Some(at) = act_type {
            let mut stmt = conn
                .prepare(
                    "SELECT a.act_name, a.act_desc, a.act_id FROM ecs_goods_activity a
                     JOIN ecs_goods g ON g.goods_id = a.goods_id
                     WHERE a.act_type = ?1 AND a.is_finished = 0 AND g.is_on_sale = 1 AND g.is_delete = 0
                     ORDER BY a.act_id DESC LIMIT 100",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([at], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?.to_string(),
                    ))
                })
                .map_err(db_err)?;
            items = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        } else {
            let sql = format!(
                "SELECT goods_name, goods_brief, goods_id FROM ecs_goods {where_clause} ORDER BY goods_id DESC LIMIT 100"
            );
            let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(params_ref.as_slice(), |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?.to_string(),
                    ))
                })
                .map_err(db_err)?;
            items = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        }
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<rss version=\"2.0\"><channel><title>{}</title><link>{}</link>",
            escape_xml("ECSHOP Feed"),
            escape_xml(&base_url)
        );
        for (title, desc, id) in items {
            xml.push_str(&format!(
                "<item><title>{}</title><description>{}</description><link>{}</link></item>",
                escape_xml(&title),
                escape_xml(&desc),
                escape_xml(&format!("{}/#/goods/{}", base_url, id))
            ));
        }
        xml.push_str("</channel></rss>");
        Ok(xml)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(xml_response(body))
}
