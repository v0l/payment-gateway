use super::*;
use payment_gateway::accounts;

pub struct Client {
    pub app: Router,
    pub cookie: String,
    pub csrf: String,
}

impl Client {
    pub async fn req(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let r = self.raw(method, path, body).await;
        (r.status, r.json)
    }

    pub async fn raw(&self, method: &str, path: &str, body: Value) -> Reply {
        let headers = [
            ("cookie", format!("pgw_session={}", self.cookie)),
            ("x-csrf-token", self.csrf.clone()),
        ];
        let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        send(
            &self.app,
            method,
            &format!("/console/api{path}"),
            None,
            &h,
            body,
        )
        .await
    }
}

pub async fn signed_in(h: &Harness, email: &str, operator: bool) -> Client {
    let hash = accounts::hash_password("correct horse battery").unwrap();
    accounts::insert_account(&h.pool, email, &hash, operator)
        .await
        .unwrap();
    let r = send(
        &h.app,
        "POST",
        "/console/api/login",
        None,
        &[],
        json!({"email": email, "password": "correct horse battery"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("pgw_session=")
        .to_string();
    Client {
        app: h.app.clone(),
        cookie,
        csrf: r.json["csrf"].as_str().unwrap_or("").to_string(),
    }
}

pub struct Project {
    pub id: u64,
    pub key: String,
    pub secret: String,
}

pub async fn project(c: &Client, name: &str) -> Project {
    let (s, p) = c.req("POST", "/projects", json!({"name": name})).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    Project {
        id: p["id"].as_u64().unwrap(),
        key: p["api_key"].as_str().unwrap().to_string(),
        secret: p["webhook_secret"].as_str().unwrap().to_string(),
    }
}

pub async fn payment(h: &Harness, p: &Project, body: Value) -> String {
    let (s, r) = call(&h.app, "POST", "/v1/payments", Some(&p.key), body).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    r["id"].as_str().unwrap().to_string()
}

pub async fn force_paid(h: &Harness, id: &str, minutes_to_settle: i64) {
    sqlx::query(
        "update payment set status = 'paid', paid_at = created + interval ? minute where id = ?",
    )
    .bind(minutes_to_settle)
    .bind(&pid(id)[..])
    .execute(&h.pool)
    .await
    .unwrap();
}

pub async fn age(h: &Harness, id: &str, hours: i64) {
    sqlx::query("update payment set created = created - interval ? hour where id = ?")
        .bind(hours)
        .bind(&pid(id)[..])
        .execute(&h.pool)
        .await
        .unwrap();
}
