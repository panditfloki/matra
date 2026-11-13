//! Synthetic contracts taken from the preserved native Mac provider tests.
//! Every path is injected and every HTTP request terminates on loopback.
use super::super::engine::Urls;
use super::super::net::testkit;
use super::*;
use std::{collections::BTreeMap, sync::Arc};

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("matra-fixed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn env(&self, vars: &[(&str, String)]) -> Env {
        let vars: BTreeMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Env {
            settings_dir: self.0.join("settings"),
            home: Some(self.0.join("home")),
            appdata: Some(self.0.join("appdata")),
            local_appdata: Some(self.0.join("local")),
            path_var: None,
            vars: Arc::new(move |k| vars.get(k).cloned()),
            urls: Urls::default(),
        }
    }
    fn put(&self, relative: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
    fn db(&self, relative: &str, sql: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        Connection::open(&path).unwrap().execute_batch(sql).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-10, "{a} != {b}");
}
fn ids(r: &Reading) -> Vec<&str> {
    r.windows.iter().map(|w| w.id.as_str()).collect()
}
fn secret() -> Credential {
    Credential {
        token: "synthetic-token".into(),
        organization: None,
        source: "fixture",
        plan: None,
    }
}

#[test]
fn devin_quota_vectors_preserve_hidden_unknown_zero_and_micros() {
    let vectors = [
        (
            json!({"dailyQuotaRemainingPercent":99.5,"dailyQuotaResetAtUnix":"1789113600","weeklyQuotaRemainingPercent":50,"overageBalanceMicros":"14277951"}),
            vec!["daily", "weekly", "overage"],
            Some(0.005),
            Some("$14.28"),
        ),
        (
            json!({"hideDailyQuota":true,"dailyQuotaRemainingPercent":99,"weeklyQuotaRemainingPercent":50}),
            vec!["weekly"],
            Some(0.5),
            None,
        ),
        (
            json!({"dailyQuotaResetAtUnix":100,"overageBalanceMicros":"0"}),
            vec!["daily", "overage"],
            Some(1.0),
            Some("$0.00"),
        ),
        (
            json!({"dailyQuotaRemainingPercent":100}),
            vec!["daily"],
            Some(0.0),
            None,
        ),
    ];
    for (plan, expected, used, money) in vectors {
        let r = devin_parse(&json!({"userStatus":{"planStatus":plan}})).unwrap();
        assert_eq!(ids(&r), expected);
        close(r.windows[0].used, used.unwrap());
        assert_eq!(
            r.details
                .as_ref()
                .and_then(|v| v["texts"]["overage"]["used_text"].as_str()),
            money
        );
    }
    for bad in [
        Value::Null,
        json!(true),
        json!(-1),
        json!(101),
        json!("NaN"),
        json!("Infinity"),
    ] {
        assert!(devin_parse(
            &json!({"userStatus":{"planStatus":{"dailyQuotaRemainingPercent":bad}}})
        )
        .is_err());
    }
    let past =
        devin_parse(&json!({"userStatus":{"planStatus":{"dailyQuotaResetAtUnix":100}}})).unwrap();
    assert_eq!(past.windows[0].resets_at, Some(100000));
    assert!(devin_parse(&json!({"userStatus":{"planStatus":{}}})).is_err());
}

#[test]
fn devin_uses_desktop_then_cli_without_reading_cached_quota_or_writing_database() {
    let f = Fixture::new("devin-credentials");
    let env = f.env(&[]);
    assert_eq!(devin_credential(&env).err().unwrap().status, "needsAuth");
    f.put(
        "home/.local/share/devin/credentials.toml",
        "windsurf_api_key = \"cli-synthetic\"\n",
    );
    assert_eq!(devin_credential(&env).unwrap().source, "Devin CLI");
    let path=f.db("appdata/Devin/User/globalStorage/state.vscdb", "CREATE TABLE ItemTable(key TEXT PRIMARY KEY,value TEXT); INSERT INTO ItemTable VALUES ('windsurfAuthStatus','{\"apiKey\":\"desktop-synthetic\"}'); INSERT INTO ItemTable VALUES ('cachedPlan','{\"dailyQuotaRemainingPercent\":100}');");
    let before = std::fs::read(&path).unwrap();
    let auth = devin_credential(&env).unwrap();
    assert_eq!(auth.token, "desktop-synthetic");
    let server = testkit::serve(vec![(
        "/user-status",
        testkit::ok(r#"{"userStatus":{"planStatus":{"dailyQuotaRemainingPercent":25}}}"#),
    )]);
    let r = devin_read(&format!("{}/user-status", server.base), &auth).unwrap();
    close(r.windows[0].used, 0.75);
    assert_eq!(std::fs::read(path).unwrap(), before);
    let request = server.requests.lock().unwrap()[0].clone();
    assert!(request.starts_with("POST /user-status"));
    assert!(request.contains("Connect-Protocol-Version: 1"));
    let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["metadata"]["apiKey"], "desktop-synthetic");
    assert_eq!(body["metadata"]["ideVersion"], "1.108.2");
    assert!(!serde_json::to_string(&r.details)
        .unwrap()
        .contains("synthetic"));
}

#[test]
fn status_vectors_keep_auth_permissions_rate_limit_and_unknown_distinct() {
    for (status, auth, expected, transient) in [
        (401, false, "needsAuth", false),
        (403, false, "error", false),
        (403, true, "needsAuth", false),
        (429, false, "backoff", true),
        (503, false, "error", true),
        (302, false, "error", false),
    ] {
        let f = http(
            Reply {
                status,
                body: b"private-token-from-server".to_vec(),
                retry_after: Some(901),
                ..Default::default()
            },
            "Fixture",
            auth,
        )
        .unwrap_err();
        assert_eq!(f.status, expected);
        assert_eq!(f.transient, transient);
        assert!(!f.note.contains("private-token"));
        if status == 429 {
            assert_eq!(f.retry_after, Some(901));
            assert!(f.rate_limited);
        }
    }
}

#[test]
fn amp_vectors_cover_current_legacy_free_zero_and_malformed_paid() {
    let vectors=[
        ("**Amp Gigawatt Tier:** agent usage $150.25 of $200 remaining (75%), orb usage 900.5h of 1,000h a1.xxlarge orb hours remaining (90%) - resets upon renewal in 25 days", "Gigawatt", "agent", 0.25),
        ("Amp Team Plan Subscription: 99.5% agent usage and 0% orb usage remaining - resets upon renewal in 1 day", "Team Plan", "agent", 0.005),
        ("Amp Megawatt Subscription: 75% other usage and 95% orb usage remaining", "Megawatt", "agent", 0.25),
        ("Amp Free: $4.50/$10.00 remaining (replenishes +$0.50/hour)","Free","free",0.55),
        ("Amp Free: $0/$10 remaining (replenishes +$0.50/hour)","Free","free",1.),
    ];
    for (display, plan, id, used) in vectors {
        for root in [
            json!({"result":{"displayText":display}}),
            json!({"displayText":display}),
        ] {
            let r = amp::parse(&root).unwrap();
            assert_eq!(r.plan.as_deref(), Some(plan));
            assert_eq!(r.windows[0].id, id);
            close(r.windows[0].used, used);
            assert!(r
                .windows
                .iter()
                .all(|w| w.resets_at.is_none() && w.duration_ms.is_none()));
            assert_eq!(r.windows[0].derived, id == "free");
        }
    }
    for root in [
        json!({"ok":false,"displayText":"Amp Free: $4/$10 remaining (replenishes +$1/hour)"}),
        json!({"error":{"message":"secret"}}),
        json!({"result":{"displayText":"Amp Team Tier: broken\nAmp Free: $4/$10 remaining (replenishes +$1/hour)"}}),
        json!({"displayText":"Amp Team Subscription: 101% agent usage and 5% orb usage remaining"}),
        json!({"displayText":"Amp Free: $11/$10 remaining (replenishes +$1/hour)"}),
    ] {
        assert!(amp::parse(&root).is_err());
    }
}

#[test]
fn amp_uses_only_the_exact_origin_key_and_its_json_rpc_protocol() {
    let f = Fixture::new("amp-origin");
    let env = f.env(&[]);
    f.put(
        "home/.local/share/amp/secrets.json",
        r#"{"apiKey@https://ampcode.com.evil/":"wrong","apiKey@https://proxy.example/":"wrong"}"#,
    );
    assert_eq!(amp_credential(&env).err().unwrap().status, "needsAuth");
    f.put(
        "home/.local/share/amp/secrets.json",
        r#"{"apiKey@https://ampcode.com/":"synthetic-token"}"#,
    );
    let c = amp_credential(&env).unwrap();
    let server = testkit::serve(vec![(
        "/internal",
        testkit::ok(
            r#"{"result":{"displayText":"Amp Free: $10/$10 remaining (replenishes +$1/hour)"}}"#,
        ),
    )]);
    close(
        amp::read(&format!("{}/internal", server.base), &c)
            .unwrap()
            .windows[0]
            .used,
        0.,
    );
    assert!(server.saw("Authorization: Bearer synthetic-token"));
    assert!(server.saw("userDisplayBalanceInfo"));
    f.put(
        "home/.local/share/amp/secrets.json",
        r#"{"apiKey@https://ampcode.com/":"bad\r\nHeader: yes"}"#,
    );
    assert!(amp_credential(&env).is_err());
    f.put("home/.local/share/amp/secrets.json", "not json");
    assert_eq!(amp_credential(&env).err().unwrap().status, "error");
}

#[test]
fn apify_vectors_use_current_spend_only_and_never_invent_a_cap() {
    let r=apify_parse(&json!({"data":{"current":{"monthlyUsageUsd":1200.60},"monthlyUsageCycle":{"startAt":"2026-09-01T00:00:00Z","endAt":"2026-10-01T00:00:00Z"}}})).unwrap();
    assert_eq!(
        r.details.unwrap()["texts"]["monthly"]["used_text"],
        "$1,200.60"
    );
    assert_eq!(r.windows[0].duration_ms, Some(30 * 86400000));
    for (used, cap, expected) in [
        (0., Some(5.), Some(0.)),
        (8., Some(5.), Some(1.6)),
        (2., None, None),
        (2., Some(0.), None),
    ] {
        let r=apify_parse(&json!({"data":{"current":{"monthlyUsageUsd":used},"limits":{"maxMonthlyUsageUsd":cap},"monthlyUsageCycle":{"endAt":"2026-10-01T00:00:00Z"},"usageMonthlyLimitUsd":99}})).unwrap();
        if let Some(n) = expected {
            close(r.windows[0].used, n);
            assert!(r.windows[0].count.is_none());
        } else {
            assert_eq!(r.windows[0].count, Some(0));
        }
        assert_eq!(
            r.details.unwrap()["texts"]["monthly"]["used_text"],
            format!("${used:0.2}")
        );
        assert_eq!(r.windows[0].resets_at, Some(1790812800000));
    }
    for bad in [Value::Null, json!(true), json!("0"), json!(-1)] {
        assert!(apify_parse(&json!({"data":{"current":{"monthlyUsageUsd":bad}}})).is_err());
    }
    assert!(apify_parse(&json!({"monthlyUsageUsd":0})).is_err());
}

#[test]
fn apify_explicit_token_does_not_borrow_another_accounts_plan() {
    let f = Fixture::new("apify-source");
    let env = f.env(&[]);
    f.put(
        "home/.apify/auth.json",
        r#"{"token":"cli-token","plan":{"id":"PERSONAL"}}"#,
    );
    let borrowed = apify_credential(&env, None).unwrap();
    assert_eq!(borrowed.plan.as_deref(), Some("Personal"));
    let explicit = apify_credential(&env, Some("explicit-token".into())).unwrap();
    assert_eq!(explicit.plan, None);
    assert_ne!(explicit.scope(), borrowed.scope());
    let server = testkit::serve(vec![(
        "/limits",
        testkit::ok(
            r#"{"data":{"current":{"monthlyUsageUsd":0},"limits":{"maxMonthlyUsageUsd":5}}}"#,
        ),
    )]);
    assert_eq!(
        apify_read(&format!("{}/limits", server.base), &explicit)
            .unwrap()
            .plan,
        None
    );
    assert!(server.saw("Authorization: Bearer explicit-token"));
    f.put(
        "home/.apify/auth.json",
        r#"{"tokenStoredInKeyring":true,"plan":{"id":"TEAM"}}"#,
    );
    let failure = apify_credential(&env, None).err().unwrap();
    assert_eq!(failure.status, "needsAuth");
    assert!(failure.note.contains("Paste"));
}

fn kilo_usage() -> Value {
    json!({"result":{"data":{"json":{"subscription":{"windows":[
        {"id":"opaque-week","remainingPercent":50,"period":{"value":1,"unit":"week"}},
        {"id":"opaque-five","remainingPercent":75,"period":{"value":5,"unit":"hour"},"resetsAt":"2026-10-01T00:00:00.123Z"},
        {"id":"opaque-month","remainingPercent":100,"period":{"value":1,"unit":"month"}}
    ]}}}}})
}

#[test]
fn kilo_protocol_keeps_organization_scope_and_server_period_order() {
    let f = Fixture::new("kilo");
    let env = f.env(&[]);
    f.put(
        "home/.local/share/kilo/auth.json",
        r#"{"kilo":{"type":"oauth","access":"synthetic-token","accountId":"org-fixture"}}"#,
    );
    let c = kilo_credential(&env).unwrap();
    assert_eq!(c.organization.as_deref(), Some("org-fixture"));
    let server = testkit::serve(vec![
        (
            "listSubscriptions",
            testkit::ok(
                r#"{"result":{"data":{"json":[{"id":"cancelled","planName":"Old","status":"canceled"},{"id":"plan-synthetic","planName":"Pro","status":"past_due"}]}}}"#,
            ),
        ),
        ("getUsage", testkit::ok(&kilo_usage().to_string())),
        ("/balance", testkit::ok(r#"{"balance":0}"#)),
    ]);
    let r = kilo_read(&server.base, &c).unwrap();
    assert_eq!(
        ids(&r),
        ["opaque-five", "opaque-week", "opaque-month", "balance"]
    );
    assert_eq!(r.plan.as_deref(), Some("Pro"));
    close(r.windows[0].used, 0.25);
    assert_eq!(r.windows[0].duration_ms, Some(18000000));
    assert_eq!(r.windows[0].resets_at, Some(1790812800123));
    assert_eq!(r.details.unwrap()["texts"]["balance"]["used_text"], "$0.00");
    let seen = server.requests.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen
        .iter()
        .all(|s| s.contains("x-kilocode-organizationid: org-fixture")));
    assert!(seen[1].contains("subscriptionId%22%3A%22plan-synthetic"));
    drop(seen);
    f.put(
        "home/.local/share/kilo/auth.json",
        r#"{"kilo":{"type":"api","key":"synthetic-token","accountId":"ignored"}}"#,
    );
    let api = kilo_credential(&env).unwrap();
    assert_eq!(api.organization, None);
    assert_ne!(c.scope(), api.scope());
}

#[test]
fn kilo_plan_auth_refusal_falls_through_to_balance_but_other_errors_do_not() {
    for path in ["listSubscriptions", "getUsage"] {
        let denied = testkit::response("403 Forbidden", "", "private");
        let server = testkit::serve(vec![
            (
                "listSubscriptions",
                if path == "listSubscriptions" {
                    denied.clone()
                } else {
                    testkit::ok(
                        r#"{"result":{"data":[{"id":"a","planName":"P","status":"active"}]}}"#,
                    )
                },
            ),
            ("getUsage", denied),
            ("/balance", testkit::ok(r#"{"balance":0}"#)),
        ]);
        let r = kilo_read(&server.base, &secret()).unwrap();
        assert_eq!(ids(&r), ["balance"]);
        assert!(server.saw("/balance"));
    }
    let server = testkit::serve(vec![
        (
            "listSubscriptions",
            testkit::response("429 Too Many Requests", "Retry-After: 700\r\n", ""),
        ),
        ("/balance", testkit::ok(r#"{"balance":12}"#)),
    ]);
    assert_eq!(
        kilo_read(&server.base, &secret()).unwrap_err().status,
        "backoff"
    );
    assert!(!server.saw("/balance"));
    for value in [
        json!({"error":{}}),
        json!({"result":{"data":{"subscription":{"windows":[]}}}}),
    ] {
        assert!(kilo_windows(&value).is_err());
    }
}

fn minimax_response(lane: Value) -> Value {
    json!({"base_resp":{"status_code":0},"model_remains":[lane]})
}
fn minimax_targets(base: &str) -> minimax::Targets {
    minimax::Targets {
        international: vec![format!("{base}/intl-missing"), format!("{base}/intl")],
        china: vec![format!("{base}/china")],
        cookie_international: format!("{base}/cookie-intl"),
        cookie_china: format!("{base}/cookie-cn"),
    }
}

#[test]
fn minimax_vectors_keep_remaining_arithmetic_lanes_reset_and_unlimited() {
    let now = 1790000000000;
    let vectors = [
        (
            json!({"model_name":"general","current_interval_total_count":100,"current_interval_usage_count":80,"remains_time":30000}),
            0.2,
            Some(now + 30000),
        ),
        (
            json!({"model_name":"Text Generation","current_interval_total_count":100,"current_interval_usage_count":1,"current_interval_remaining_percent":"75","interval_boost_permill":2000,"start_time":1790000000,"end_time":1790018000}),
            0.25,
            Some(1790018000000),
        ),
        (
            json!({"model_name":"MiniMax-M2.5","current_interval_remaining_percent":125,"end_time":100,"remains_time":20}),
            0.,
            Some(now + 20),
        ),
    ];
    for (lane, used, reset) in vectors {
        let r = minimax::parse(&minimax_response(lane), now).unwrap();
        close(r.windows[0].used, used);
        assert_eq!(r.windows[0].resets_at, reset);
        assert_eq!(r.windows[0].duration_ms, Some(18000000));
    }
    let mixed = json!({"data":{"model_remains":[{"model_name":"video","current_interval_remaining_percent":0},{"model_name":"M2.5","current_interval_remaining_percent":50},{"model_name":"general","current_interval_remaining_percent":75,"current_weekly_status":3,"current_weekly_remaining_percent":100}]}});
    let r = minimax::parse(&mixed, now).unwrap();
    close(r.windows[0].used, 0.25);
    close(r.windows[1].used, 0.);
    assert_eq!(r.windows[1].resets_at, None);
    for lane in [
        json!({"model_name":"video","current_interval_remaining_percent":0}),
        json!({"model_name":"general","current_interval_status":3,"current_interval_total_count":0,"current_interval_usage_count":0,"current_interval_remaining_percent":100}),
        json!({"model_name":"general","current_interval_remaining_percent":"NaN"}),
    ] {
        assert_eq!(
            minimax::parse(&minimax_response(lane), now)
                .unwrap_err()
                .status,
            "none"
        );
    }
}

#[test]
fn minimax_region_fallback_is_for_bearer_auth_only_and_limit_has_precedence() {
    let good =
        minimax_response(json!({"model_name":"general","current_interval_remaining_percent":60}))
            .to_string();
    let server = testkit::serve(vec![
        (
            "/intl-missing",
            testkit::ok(r#"{"base_resp":{"status_code":1004}}"#),
        ),
        ("/intl", testkit::response("401 Unauthorized", "", "")),
        ("/china", testkit::ok(&good)),
        (
            "/cookie-intl",
            testkit::response("401 Unauthorized", "", ""),
        ),
        ("/cookie-cn", testkit::ok(&good)),
    ]);
    let targets = minimax_targets(&server.base);
    let r = minimax::read(
        &targets,
        "international",
        Some("Bearer plan-key".into()),
        None,
        1,
    )
    .unwrap();
    close(r.windows[0].used, 0.4);
    assert_eq!(server.count(), 3);
    assert!(server.saw("Authorization: Bearer plan-key"));
    assert_eq!(
        minimax::read(
            &targets,
            "international",
            None,
            Some("session=synthetic".into()),
            1
        )
        .unwrap_err()
        .status,
        "needsAuth"
    );
    assert_eq!(server.count(), 4);
    assert!(!server.saw("/cookie-cn"));
    let limit = testkit::serve(vec![(
        "/intl-missing",
        testkit::response(
            "200 OK",
            "Retry-After: 1200\r\n",
            r#"{"base_resp":{"status_code":1004},"data":{"base_resp":{"status_code":2045}}}"#,
        ),
    )]);
    let err = minimax::read(
        &minimax_targets(&limit.base),
        "international",
        Some("key".into()),
        None,
        1,
    )
    .unwrap_err();
    assert_eq!(err.status, "backoff");
    assert_eq!(err.retry_after, Some(1200));
    assert_eq!(limit.count(), 1);
}

#[test]
fn minimax_cookie_normalization_never_opens_a_browser_store_or_falls_into_payg() {
    for raw in [
        "session=synthetic; foo=bar",
        "Cookie: session=synthetic; foo=bar",
        "curl 'https://www.minimax.io/' -H 'Cookie: session=synthetic; foo=bar'",
        "curl x --cookie \"session=synthetic; foo=bar\"",
    ] {
        assert_eq!(
            minimax::cookie(raw).as_deref(),
            Some("session=synthetic; foo=bar")
        );
    }
    for raw in [
        "/path/to/cookies.sqlite",
        "curl -b /path/to/file",
        "value\nHeader: injected",
        "cookiewithoutvalue",
    ] {
        assert!(minimax::cookie(raw).is_none());
    }
    assert_eq!(minimax::coding_key(Some("bearer sk-api-payg".into())), None);
    assert_eq!(
        minimax::coding_key(Some("Bearer plan-synthetic".into())),
        Some("plan-synthetic".into())
    );
}

#[test]
fn kiro_cli_vectors_keep_zero_plan_only_bonus_and_auth_distinct() {
    let now = 1790000000000;
    for (input,plan,expected) in [
        ("| KIRO FREE |\n████ 25%\n(12.50 of 50 covered in plan), resets on 2026-10-01","Kiro Free",Some(0.25)),
        ("\u{1b}[1mEstimated Usage\u{1b}[0m | resets on 2026-10-01 | KIRO FREE\nCredits (0.17 of 50 covered in plan)\n████ 0%","Kiro Free",Some(0.)),
        ("Plan: KIRO PRO MAX | 1 usage breakdowns","Kiro Pro Max",None),
        ("Plan: Q Developer Pro\nYour plan is managed by admin","Q Developer Pro",None),
    ] {
        let r=kiro::parse(input,now).unwrap();assert_eq!(r.plan.as_deref(),Some(plan));
        match expected { Some(v)=>close(r.windows[0].used,v), None=>assert!(r.windows.is_empty()) }
    }
    let r=kiro::parse("Plan: KIRO PRO\n████ 80%\nBonus credits: 5.00/10 credits used, expires in 7 days\nOverages: Disabled",now).unwrap();
    assert_eq!(ids(&r), ["credits", "bonus", "overage-status"]);
    close(r.windows[1].used, 0.5);
    assert_eq!(r.windows[1].group.as_deref(), Some("Bonus"));
    for input in [
        "Not logged in.",
        "login required",
        "Failed to initialize auth portal.",
        "kiro-cli login --use-device-flow",
        "OAuth error",
    ] {
        assert_eq!(kiro::parse(input, now).unwrap_err().status, "needsAuth");
    }
    assert_eq!(
        kiro::parse("unrecognized output", now).unwrap_err().status,
        "none"
    );
}

#[test]
fn kiro_enrichment_subtracts_overage_preserves_bonus_and_rejects_ambiguous_limits() {
    let mut r = kiro::parse(
        "Plan: KIRO PRO\n████ 80%\nBonus credits: 5/10 credits used, expires in 7 days",
        1,
    )
    .unwrap();
    let payload = json!({"overageConfiguration":{"overageStatus":"ENABLED"},"usageBreakdownList":[{"resourceType":"CREDIT","usageLimitWithPrecision":100,"currentUsageWithPrecision":105.5,"currentOveragesWithPrecision":10.5,"overageCapWithPrecision":50,"nextDateReset":1790812800}]});
    kiro::apply_limits(&mut r, &payload).unwrap();
    assert_eq!(ids(&r), ["credits", "bonus", "overage"]);
    close(r.windows[0].used, 0.95);
    close(r.windows[1].used, 0.5);
    close(r.windows[2].used, 0.21);
    let mut bonus = payload.clone();
    bonus["usageBreakdownList"][0]["bonuses"] = json!([{"currentUsage":100}]);
    bonus["usageBreakdownList"][0]["currentUsageWithPrecision"] = json!(200);
    kiro::apply_limits(&mut r, &bonus).unwrap();
    close(r.windows[0].used, 0.95);
    for bad in [
        json!({}),
        json!({"usageBreakdownList":[{"resourceType":"CREDIT"},{"resourceType":"CREDIT"}]}),
        json!({"usageBreakdownList":[{"resourceType":"CREDIT","usageLimit":100,"currentUsage":105}]}),
        json!({"usageBreakdownList":[{"resourceType":"CREDIT","usageLimit":100,"currentUsage":0,"currentOverages":1}]}),
    ] {
        assert!(kiro::apply_limits(&mut r, &bad).is_err());
    }
    let mut unknown = Reading::default();
    kiro::apply_limits(&mut unknown,&json!({"overageConfiguration":{"overageStatus":"ENABLED"},"usageBreakdownList":[{"resourceType":"CREDIT","usageLimit":100,"currentUsage":0}]})).unwrap();
    assert_eq!(ids(&unknown), ["credits"]);
    close(unknown.windows[0].used, 0.);
}

#[test]
fn kiro_sqlite_is_read_only_and_profile_region_is_allowlisted() {
    let f = Fixture::new("kiro-credential");
    let dir = f.0.join("kiro-state");
    let env = f.env(&[("KIRO_DATA_DIR", dir.to_string_lossy().into_owned())]);
    let path=f.db("kiro-state/data.sqlite3","CREATE TABLE auth_kv(key TEXT,value TEXT); CREATE TABLE state(key TEXT,value TEXT); INSERT INTO auth_kv VALUES('kirocli:odic:token','{\"access_token\":\"synthetic\"}'); INSERT INTO state VALUES('api.codewhisperer.profile','{\"arn\":\"arn:aws:codewhisperer:eu-central-1:123:profile/example\"}');");
    let before = std::fs::read(&path).unwrap();
    let (token, arn) = kiro::credential(&env).unwrap();
    assert_eq!(token, "synthetic");
    assert_eq!(
        kiro::endpoint(&arn),
        Some("https://q.eu-central-1.amazonaws.com/")
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(database(&path)
        .unwrap()
        .execute("DELETE FROM auth_kv", [])
        .is_err());
    for arn in [
        "arn:aws:codewhisperer:ap-south-1:123:profile/a",
        "arn:aws:evil:us-east-1:123:profile/a",
        "arn:aws:codewhisperer:us-east-1::profile/a",
        "arn:aws:codewhisperer:us-east-1:123:profile/",
        "arn:aws:codewhisperer:us-east-1:123:profile/a\nHeader: injected",
    ] {
        assert!(kiro::endpoint(arn).is_none());
    }
}

#[cfg(unix)]
#[test]
fn kiro_process_selects_usage_stream_checks_exit_and_bounds_output_and_process_lifetime() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let f = Fixture::new("kiro-process");
    let script = |name: &str, body: &str| {
        let p = f.put(name, format!("#!/bin/sh\n{body}\n"));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    };
    let p=script("ok","[ \"$1 $2 $3\" = 'chat --no-interactive /usage' ] || exit 9\n[ \"$TERM $KIRO_CHAT_UI\" = 'dumb classic' ] || exit 9\necho warning\necho 'Estimated Usage | KIRO FREE' >&2");
    assert_eq!(
        kiro::run(&p, &f.0.join("poll"), Duration::from_secs(2))
            .unwrap()
            .trim(),
        "Estimated Usage | KIRO FREE"
    );
    let p = script("bad", "echo 'Estimated Usage'; exit 2");
    assert_eq!(
        kiro::run(&p, &f.0.join("poll"), Duration::from_secs(2))
            .unwrap_err()
            .status,
        "needsAuth"
    );
    let p = script("slow", "sleep 20 &\nwait");
    let start = Instant::now();
    assert!(kiro::run(&p, &f.0.join("poll"), Duration::from_millis(100)).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    let p = script("huge", "/usr/bin/head -c 70000 /dev/zero");
    assert!(kiro::run(&p, &f.0.join("poll"), Duration::from_secs(2)).is_err());
    let env = f.env(&[(
        "KIRO_CLI_PATH",
        f.0.join("does-not-exist").to_string_lossy().into_owned(),
    )]);
    assert!(
        kiro::locate(&env).is_none(),
        "An explicit missing CLI must not fall through to a real installation"
    );
}

fn period() -> gemini::Period {
    gemini::Period {
        start: 1788220800000,
        day: 1789430400000,
        end: 1790812800000,
        next_day: 1789516800000,
    }
}

#[test]
fn gemini_jsonl_last_record_wins_and_cache_thoughts_and_rewind_are_not_added_again() {
    let lines=[
        json!({"id":"a","type":"gemini","timestamp":"2026-09-15T10:00:00Z","tokens":{"total":100,"cached":10,"thoughts":50}}),
        json!({"id":"a","type":"gemini","timestamp":"2026-09-15T10:00:00Z","tokens":{"total":120,"cached":10,"thoughts":50}}),
        json!({"$rewindTo":"a"}),
        json!({"id":"b","type":"gemini","timestamp":"2026-09-03T10:00:00Z","tokens":{"total":300}}),
        json!({"id":"old","type":"gemini","timestamp":"2026-08-15T10:00:00Z","tokens":{"total":999}}),
        json!({"id":"future","type":"gemini","timestamp":"2026-10-01T00:00:00Z","tokens":{"total":999}}),
        json!({"id":"zero","type":"gemini","timestamp":"2026-09-15T10:00:00Z","tokens":{"total":0}}),
        json!({"id":"user","type":"user","timestamp":"2026-09-15T10:00:00Z","tokens":{"total":900}}),
    ].map(|v|v.to_string()).join("\n");
    assert_eq!(
        gemini::cli_text(&lines, period()),
        gemini::Totals {
            month: 420,
            today: 120,
            calls: 2
        }
    );
}

#[test]
fn gemini_opencode_prefers_v2_and_authoritative_total_then_component_sum() {
    let f = Fixture::new("gemini-opencode");
    let p = period();
    let v1 = f.db(
        "v1.db",
        "CREATE TABLE message(time_created INTEGER,data TEXT);",
    );
    let db = Connection::open(&v1).unwrap();
    for (provider, tokens) in [
        (
            "google",
            json!({"total":100,"input":500,"output":40,"cache":{"read":80}}),
        ),
        (
            "google",
            json!({"input":10,"output":20,"reasoning":3,"cache":{"read":4,"write":5}}),
        ),
        ("google-vertex", json!({"total":999})),
    ] {
        db.execute(
            "INSERT INTO message VALUES(?1,?2)",
            rusqlite::params![
                p.day + 1000,
                json!({"role":"assistant","providerID":provider,"tokens":tokens}).to_string()
            ],
        )
        .unwrap();
    }
    assert_eq!(
        gemini::opencode(&v1, p),
        Some(gemini::Totals {
            month: 142,
            today: 142,
            calls: 2
        })
    );
    db.execute_batch("CREATE TABLE session_v2(id TEXT); CREATE TABLE session_message(type TEXT,time_created INTEGER,data TEXT);").unwrap();
    db.execute(
        "INSERT INTO session_message VALUES('assistant',?1,?2)",
        rusqlite::params![
            p.day + 1000,
            json!({"model":{"providerID":"google"},"tokens":{"total":25}}).to_string()
        ],
    )
    .unwrap();
    assert_eq!(
        gemini::opencode(&v1, p),
        Some(gemini::Totals {
            month: 25,
            today: 25,
            calls: 1
        }),
        "Migrated v1 rows must not be billed twice"
    );
}

#[test]
fn gemini_hermes_counts_billed_gemini_cache_once_and_preserves_call_count() {
    let f = Fixture::new("gemini-hermes");
    let p = period();
    let path=f.db("hermes.db","CREATE TABLE session_model_usage(billing_provider TEXT,last_seen REAL,input_tokens INTEGER,cache_read_tokens INTEGER,cache_write_tokens INTEGER,output_tokens INTEGER,reasoning_tokens INTEGER,api_call_count INTEGER);");
    let db = Connection::open(&path).unwrap();
    for (provider, at) in [
        ("gemini", p.day / 1000 + 1),
        ("vertex", p.day / 1000 + 1),
        ("gemini", p.start / 1000 - 1),
    ] {
        db.execute(
            "INSERT INTO session_model_usage VALUES(?1,?2,10,20,30,40,900,7)",
            rusqlite::params![provider, at],
        )
        .unwrap();
    }
    assert_eq!(
        gemini::hermes(&path, p),
        Some(gemini::Totals {
            month: 100,
            today: 100,
            calls: 7
        })
    );
}

#[test]
fn gemini_empty_missing_and_budgeted_histories_remain_distinct() {
    let f = Fixture::new("gemini-snapshot");
    let env = f.env(&[]);
    let now = 1789473600000;
    assert_eq!(gemini::read(&env, None, now).unwrap_err().status, "none");
    f.put(
        "home/.gemini/tmp/project/chats/a.jsonl",
        r#"{"id":"a","type":"gemini","timestamp":"2026-09-15T10:00:00Z","tokens":{"total":120}}"#,
    );
    for budget in [None, Some(0), Some(100)] {
        let r = gemini::read(&env, budget, now).unwrap();
        assert_eq!(ids(&r), ["month", "today", "cli"]);
        assert!(r.windows.iter().all(|w| w.derived));
        if budget == Some(100) {
            close(r.windows[0].used, 1.2);
            assert_eq!(r.windows[0].count, None);
        } else {
            assert_eq!(r.windows[0].count, Some(120));
        }
        assert!(r.note.contains("not a Google quota or bill"));
        assert_eq!(r.details.unwrap()["calls_this_month"], 1);
    }
    f.put(
        "home/.gemini/tmp/project/chats/a.jsonl",
        "{\"sessionId\":\"empty\"}",
    );
    assert_eq!(
        gemini::read(&env, None, now).unwrap().windows[0].count,
        Some(0)
    );
}

#[test]
fn aws_media_type_is_not_replaced_by_the_http_adapter() {
    let server = testkit::serve(vec![("/", testkit::ok("{}"))]);
    request(
        "POST",
        &server.base,
        vec![
            net::header("Content-Type", "application/x-amz-json-1.0"),
            net::header("X-Amz-Target", "AmazonCodeWhispererService.GetUsageLimits"),
        ],
        Some("{}"),
        "Kiro",
        true,
    )
    .unwrap();
    assert!(server.saw("Content-Type: application/x-amz-json-1.0"));
    assert!(server.saw("X-Amz-Target: AmazonCodeWhispererService.GetUsageLimits"));
}

#[cfg(unix)]
#[test]
fn kiro_full_reader_keeps_cli_on_enrichment_failure_and_persists_profile_scoped_backoff() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new("kiro-full");
    let binary = f.put(
        "kiro-fixture",
        "#!/bin/sh\necho 'Plan: KIRO PRO'\necho '████ 25%'\n",
    );
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path=f.db("state/data.sqlite3","CREATE TABLE auth_kv(key TEXT,value TEXT); CREATE TABLE state(key TEXT,value TEXT); INSERT INTO auth_kv VALUES('kirocli:odic:token','{\"accessToken\":\"synthetic-token\"}'); INSERT INTO state VALUES('api.codewhisperer.profile','{\"arn\":\"arn:aws:codewhisperer:us-east-1:123:profile/a\"}');");
    let mut env = f.env(&[
        ("KIRO_CLI_PATH", binary.to_string_lossy().into_owned()),
        (
            "KIRO_DATA_DIR",
            path.parent().unwrap().to_string_lossy().into_owned(),
        ),
    ]);
    let server = testkit::serve(vec![(
        "/",
        testkit::response("429 Too Many Requests", "Retry-After: 500\r\n", ""),
    )]);
    env.urls.kiro_us = server.base.clone();
    close(
        kiro::read(&env, 1790000000000).unwrap().windows[0].used,
        0.25,
    );
    close(
        kiro::read(&env, 1790000000100).unwrap().windows[0].used,
        0.25,
    );
    assert_eq!(server.count(), 1);
    assert!(server.saw("Authorization: Bearer synthetic-token"));
    assert!(server.saw("profileArn"));
    let persisted =
        std::fs::read_to_string(env.settings_dir.join("kiro-enrichment-backoff.json")).unwrap();
    assert!(!persisted.contains("synthetic-token"));
    assert!(!persisted.contains("profile/a"));
    assert_eq!(
        serde_json::from_str::<Value>(&persisted).unwrap()["until"],
        1790000500000_u64
    );
    Connection::open(path)
        .unwrap()
        .execute(
            "UPDATE state SET value=?1",
            [r#"{"arn":"arn:aws:codewhisperer:us-east-1:123:profile/b"}"#],
        )
        .unwrap();
    close(
        kiro::read(&env, 1790000000200).unwrap().windows[0].used,
        0.25,
    );
    assert_eq!(server.count(), 2);
    let good = testkit::serve(vec![(
        "/",
        testkit::ok(
            r#"{"usageBreakdownList":[{"resourceType":"CREDIT","usageLimit":100,"currentUsage":0}]}"#,
        ),
    )]);
    env.urls.kiro_us = good.base;
    close(
        kiro::read(&env, 1790001000000).unwrap().windows[0].used,
        0.0,
    );
    assert!(!env
        .settings_dir
        .join("kiro-enrichment-backoff.json")
        .exists());
}
