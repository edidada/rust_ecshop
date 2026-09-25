//! Widgets and XML endpoints: ads, goods widget JS, cycle-image.xml, sitemap.xml, feed.xml.
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_i64, col_str, e, q, q1};
use crate::shared::util::{cents_to_string, parse_i64_param, unix_now};

fn xml_response(body: String) -> Response {
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
        .into_response()
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
    let row = q1(
        state.rb,
        "SELECT ad_id AS ad_id, media_type AS media_type, ad_name AS ad_name, ad_link AS ad_link, ad_code AS ad_code, click_count AS click_count
         FROM ecs_ad
         WHERE ad_id = ? AND enabled = 1 AND start_time <= ? AND end_time > ?",
        vec![json!(ad_id), json!(now), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("ad not found".to_string()))?;
    Ok(Json(json!({
        "id": col_i64(&row, "ad_id"),
        "media_type": col_i64(&row, "media_type"),
        "name": col_str(&row, "ad_name"),
        "link": col_str(&row, "ad_link"),
        "code": col_str(&row, "ad_code"),
        "click_count": col_i64(&row, "click_count"),
    })))
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
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let link = q1(
        &tx,
        "SELECT ad_link AS ad_link FROM ecs_ad WHERE ad_id = ? AND enabled = 1 AND start_time <= ? AND end_time > ?",
        vec![json!(ad_id), json!(now), json!(now)],
    )
    .await?
    .map(|r| col_str(&r, "ad_link"))
    .ok_or_else(|| AppError::NotFound("ad not found".to_string()))?;
    e(
        &tx,
        "UPDATE ecs_ad SET click_count = click_count + 1 WHERE ad_id = ?",
        vec![json!(ad_id)],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_adsense (from_ad, referer, clicks) VALUES (?, ?, 1)
         ON CONFLICT(from_ad, referer) DO UPDATE SET clicks = clicks + 1",
        vec![json!(ad_id), json!(body.referer)],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"redirect_url": link})))
}

/// GET /cycle-image.xml — image ads (media_type=0) as bcaster XML.
pub async fn cycle_image(State(state): State<AppState>) -> Result<Response, AppError> {
    let now = unix_now();
    let rows = q(
        state.rb,
        "SELECT ad_id AS ad_id, ad_link AS ad_link, ad_code AS ad_code FROM ecs_ad
         WHERE media_type = 0 AND enabled = 1 AND start_time <= ? AND end_time > ?
         ORDER BY position_id, ad_id",
        vec![json!(now), json!(now)],
    )
    .await?;
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<bcaster>");
    for row in rows {
        xml.push_str(&format!(
            "<item id=\"{}\" link=\"{}\">{}</item>",
            col_i64(&row, "ad_id"),
            escape_xml(&col_str(&row, "ad_link")),
            escape_xml(&col_str(&row, "ad_code"))
        ));
    }
    xml.push_str("</bcaster>");
    Ok(xml_response(xml))
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
    Query(qq): Query<WidgetQuery>,
) -> Result<Response, AppError> {
    let cat_id = parse_i64_param(qq.cat_id.as_deref(), "cat_id")?;
    let brand_id = parse_i64_param(qq.brand_id.as_deref(), "brand_id")?;
    let goods_num = parse_i64_param(qq.goods_num.as_deref(), "goods_num")?.unwrap_or(10);
    if !(1..=50).contains(&goods_num) {
        return Err(AppError::Validation("goods_num must be between 1 and 50".to_string()));
    }
    let intro_type = qq.intro_type.clone().unwrap_or_else(|| "is_new".to_string());
    if !matches!(
        intro_type.as_str(),
        "is_best" | "is_new" | "is_hot" | "is_promote" | "is_random"
    ) {
        return Err(AppError::Validation(
            "intro_type must be is_best|is_new|is_hot|is_promote|is_random".to_string(),
        ));
    }
    let mut conditions = vec!["is_on_sale = 1".to_string(), "is_delete = 0".to_string()];
    let mut args: Vec<Value> = Vec::new();
    if let Some(cid) = cat_id {
        conditions.push("cat_id = ?".to_string());
        args.push(json!(cid));
    }
    if let Some(bid) = brand_id {
        conditions.push("brand_id = ?".to_string());
        args.push(json!(bid));
    }
    let order = match intro_type.as_str() {
        "is_new" => "ORDER BY goods_id DESC".to_string(),
        "is_random" => "ORDER BY RANDOM()".to_string(),
        flag => format!("AND {flag} = 1 ORDER BY goods_id DESC"),
    };
    args.push(json!(goods_num));
    let rows = q(
        state.rb,
        &format!(
            "SELECT goods_id AS goods_id, goods_name AS goods_name, shop_price AS shop_price
             FROM ecs_goods WHERE {} {} LIMIT ?",
            conditions.join(" AND "),
            order
        ),
        args,
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "goods_id"),
            "name": col_str(&r, "goods_name"),
            "price": cents_to_string((col_str(&r, "shop_price").trim().parse::<f64>().unwrap_or(0.0) * 100.0).round() as i64),
        })
    })
    .collect::<Vec<_>>();
    let payload = json!({"intro_type": intro_type, "goods": rows});
    let js = format!(
        "window.cppEcshopGoodsWidget = {};",
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
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
    let rb = state.rb;
    let mut urls: Vec<String> = vec![format!("{}/", base_url)];
    for row in q(rb, "SELECT cat_id AS cat_id FROM ecs_category WHERE is_show = 1 ORDER BY cat_id", vec![]).await? {
        urls.push(format!("{}/#/category/{}", base_url, col_i64(&row, "cat_id")));
    }
    for row in q(rb, "SELECT cat_id AS cat_id FROM ecs_article_cat WHERE is_show = 1 ORDER BY cat_id", vec![]).await? {
        urls.push(format!("{}/#/article-cat/{}", base_url, col_i64(&row, "cat_id")));
    }
    for row in q(
        rb,
        "SELECT goods_id AS goods_id FROM ecs_goods WHERE is_on_sale = 1 AND is_delete = 0 ORDER BY goods_id LIMIT 300",
        vec![],
    )
    .await?
    {
        urls.push(format!("{}/#/goods/{}", base_url, col_i64(&row, "goods_id")));
    }
    for row in q(
        rb,
        "SELECT article_id AS article_id FROM ecs_article WHERE is_open = 1 ORDER BY article_id LIMIT 300",
        vec![],
    )
    .await?
    {
        urls.push(format!("{}/#/article/{}", base_url, col_i64(&row, "article_id")));
    }
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">",
    );
    for u in urls {
        xml.push_str(&format!("<url><loc>{}</loc></url>", escape_xml(&u)));
    }
    xml.push_str("</urlset>");
    Ok(xml_response(xml))
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
    Query(qq): Query<FeedQuery>,
) -> Result<Response, AppError> {
    let base_url = state.config.site_base_url.clone();
    let rb = state.rb;
    let cat = parse_i64_param(qq.cat.as_deref(), "cat")?;
    let brand = parse_i64_param(qq.brand.as_deref(), "brand")?;
    let feed_type = qq.feed_type.clone().unwrap_or_default();
    let mut items: Vec<(String, String, String)> = Vec::new();
    if feed_type.starts_with("article_cat") {
        let cat_id: i64 = feed_type.trim_start_matches("article_cat").parse().unwrap_or(0);
        for row in q(
            rb,
            "SELECT title AS title, article_desc AS article_desc, article_id AS article_id
             FROM ecs_article WHERE cat_id = ? AND is_open = 1 ORDER BY article_id DESC LIMIT 100",
            vec![json!(cat_id)],
        )
        .await?
        {
            items.push((
                col_str(&row, "title"),
                col_str(&row, "article_desc"),
                col_i64(&row, "article_id").to_string(),
            ));
        }
    } else {
        let act_type: Option<i64> = match feed_type.as_str() {
            "group_buy" => Some(1),
            "snatch" => Some(0),
            "auction" => Some(2),
            "exchange" => Some(5),
            "package" => Some(4),
            _ => None,
        };
        if let Some(at) = act_type {
            for row in q(
                rb,
                "SELECT a.act_name AS act_name, a.act_desc AS act_desc, a.act_id AS act_id
                 FROM ecs_goods_activity a
                 JOIN ecs_goods g ON g.goods_id = a.goods_id
                 WHERE a.act_type = ? AND a.is_finished = 0 AND g.is_on_sale = 1 AND g.is_delete = 0
                 ORDER BY a.act_id DESC LIMIT 100",
                vec![json!(at)],
            )
            .await?
            {
                items.push((
                    col_str(&row, "act_name"),
                    col_str(&row, "act_desc"),
                    col_i64(&row, "act_id").to_string(),
                ));
            }
        } else {
            let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
            let mut args: Vec<Value> = Vec::new();
            if let Some(cid) = cat {
                where_clause.push_str(" AND cat_id = ?");
                args.push(json!(cid));
            }
            if let Some(bid) = brand {
                where_clause.push_str(" AND brand_id = ?");
                args.push(json!(bid));
            }
            for row in q(
                rb,
                &format!(
                    "SELECT goods_name AS goods_name, goods_brief AS goods_brief, goods_id AS goods_id
                     FROM ecs_goods {where_clause} ORDER BY goods_id DESC LIMIT 100"
                ),
                args,
            )
            .await?
            {
                items.push((
                    col_str(&row, "goods_name"),
                    col_str(&row, "goods_brief"),
                    col_i64(&row, "goods_id").to_string(),
                ));
            }
        }
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
    Ok(xml_response(xml))
}

#[allow(dead_code)]
async fn _touch(rb: &rbatis::rbatis::RBatis) -> Result<(), AppError> {
    e(rb, "SELECT 1", vec![]).await?;
    Ok(())
}

#[allow(dead_code)]
fn _unused() {}
