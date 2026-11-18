//! 常用指令選單：清單只改這裡；範本用 <欄位> 占位，後端組字串並驗證（1.x 的 COMMANDS／build_command）。
//! danger：""＝直接送、"confirm"＝要確認、"strong"＝強確認（畫面要再打一次對象或按住 1 秒）。
use crate::settings::py_str;
use crate::{Result, UserError};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

/// 遊戲規則的舊名 → 1.21.11 起的新名（對過 Minecraft Wiki「Game rule」頁）；None＝新版已移除、隱藏。
const GAMERULE_NEW: [(&str, Option<&str>); 9] = [
    ("keepInventory", Some("keep_inventory")),
    ("doMobSpawning", Some("spawn_mobs")),
    ("mobGriefing", Some("mob_griefing")),
    ("playersSleepingPercentage", Some("players_sleeping_percentage")),
    ("doInsomnia", Some("spawn_phantoms")),
    ("doImmediateRespawn", Some("immediate_respawn")),
    ("doDaylightCycle", Some("advance_time")),
    ("doWeatherCycle", Some("advance_weather")),
    ("doFireTick", None),
];
const RULE_RENAME_FROM: [u64; 3] = [1, 21, 11];

pub const ITEMS: [(&str, &str); 9] = [
    ("diamond", "鑽石"), ("iron_ingot", "鐵錠"), ("gold_ingot", "金錠"), ("bread", "麵包"), ("cooked_beef", "牛排"),
    ("torch", "火把"), ("oak_log", "橡木原木"), ("elytra", "鞘翅"), ("totem_of_undying", "不死圖騰"),
];
pub const MOBS: [(&str, &str); 8] = [
    ("horse", "馬"), ("wolf", "狼"), ("cat", "貓"), ("villager", "村民"), ("iron_golem", "鐵魔像"),
    ("chicken", "雞"), ("cow", "牛"), ("sheep", "羊"),
];
pub const EFFECTS: [(&str, &str); 6] = [
    ("night_vision", "夜視"), ("speed", "加速"), ("haste", "挖掘加速"), ("fire_resistance", "抗火"),
    ("water_breathing", "水下呼吸"), ("regeneration", "回復"),
];
pub const ENCHANTS: [(&str, &str, i64); 40] = [
    ("protection", "保護", 4), ("fire_protection", "火焰保護", 4), ("blast_protection", "爆炸保護", 4),
    ("projectile_protection", "投射物保護", 4), ("feather_falling", "輕盈", 4), ("respiration", "水中呼吸", 3),
    ("aqua_affinity", "親水性", 1), ("thorns", "尖刺", 3), ("depth_strider", "深海漫遊", 3), ("frost_walker", "冰霜行者", 2),
    ("soul_speed", "靈魂疾走", 3), ("swift_sneak", "迅捷潛行", 3), ("sharpness", "鋒利", 5), ("smite", "不死剋星", 5),
    ("bane_of_arthropods", "節肢剋星", 5), ("knockback", "擊退", 2), ("fire_aspect", "燃燒", 2), ("looting", "掠奪", 3),
    ("sweeping_edge", "橫掃之刃", 3), ("efficiency", "效率", 5), ("silk_touch", "絲綢之觸", 1), ("unbreaking", "耐久", 3),
    ("fortune", "幸運", 3), ("power", "強力", 5), ("punch", "衝擊", 2), ("flame", "火焰", 1), ("infinity", "無限", 1),
    ("luck_of_the_sea", "海洋的祝福", 3), ("lure", "魚餌", 3), ("loyalty", "忠誠", 3), ("impaling", "魚叉", 5),
    ("riptide", "波濤", 3), ("channeling", "喚雷", 1), ("multishot", "分裂箭", 1), ("quick_charge", "快速上弦", 3),
    ("piercing", "貫穿", 4), ("density", "密度", 5), ("breach", "破甲", 4), ("wind_burst", "風爆", 3), ("mending", "修補", 1),
];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Player,
    Id,
    Ench,
    Int(i64, i64, i64), // 最小、最大、預設
    Num,
    Text,
    Bool,
}

/// (欄位, 畫面標籤, 種類, 可空白)；順序就是錯誤訊息裡列出可用欄位的順序。
const FIELDS: [(&str, &str, Kind, bool); 17] = [
    ("player", "玩家", Kind::Player, false),
    ("target", "目標玩家", Kind::Player, false),
    ("item", "物品", Kind::Id, false),
    ("mob", "生物", Kind::Id, false),
    ("effect", "效果", Kind::Id, false),
    ("ench", "附魔", Kind::Ench, false),
    ("count", "數量", Kind::Int(1, 640, 1), false),
    ("secs", "秒數", Kind::Int(1, 3600, 60), false),
    ("amp", "等級", Kind::Int(1, 10, 1), false), // 畫面 1＝I 級，送出減 1
    ("level", "附魔等級", Kind::Int(1, 5, 1), false),
    ("x", "X", Kind::Num, false),
    ("y", "Y", Kind::Num, false),
    ("z", "Z", Kind::Num, false),
    ("msg", "訊息", Kind::Text, false),
    ("reason", "原因（可空白）", Kind::Text, true),
    ("bool", "開／關", Kind::Bool, false),
    ("pct", "百分比", Kind::Int(0, 100, 100), false),
];

fn field(key: &str) -> Option<&'static (&'static str, &'static str, Kind, bool)> {
    FIELDS.iter().find(|f| f.0 == key)
}

/// 給畫面的欄位表（1.x /api/commands 的 fields）。
pub fn fields_json() -> Value {
    let picks = |list: &[(&str, &str)]| json!(list.iter().map(|(k, n)| json!([k, n])).collect::<Vec<_>>());
    let mut out = Map::new();
    for (key, label, kind, optional) in FIELDS {
        let mut f = json!({"label": label});
        let (k, extra) = match kind {
            Kind::Player => ("player", json!({})),
            Kind::Id => ("id", json!({"picks": match key { "item" => picks(&ITEMS), "mob" => picks(&MOBS), _ => picks(&EFFECTS) }})),
            Kind::Ench => ("ench", json!({"picks": ENCHANTS.iter().map(|(k, n, lv)| json!([k, n, lv])).collect::<Vec<_>>()})),
            Kind::Int(min, max, default) => ("int", json!({"min": min, "max": max, "default": default})),
            Kind::Num => ("num", json!({})),
            Kind::Text => ("text", json!({})),
            Kind::Bool => ("bool", json!({})),
        };
        f["kind"] = json!(k);
        f.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        if optional {
            f["optional"] = json!(true);
        }
        out.insert(key.into(), f);
    }
    Value::Object(out)
}

/// 內建 55 條指令（欄位與 1.x 一樣：id、cat、name、tpl、danger，再加 rule／note／confirm_if_false／special）。
pub fn builtin() -> Vec<Value> {
    let c = |id: &str, cat: &str, name: &str, tpl: &str, danger: &str, extra: Value| {
        let mut v = json!({"id": id, "cat": cat, "name": name, "tpl": tpl, "danger": danger});
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        v
    };
    let no = json!({});
    let rule = |r: &str| json!({"rule": r});
    let note = |n: &str| json!({"note": n});
    vec![
        c("day", "時間天氣", "白天", "time set day", "", no.clone()), c("noon", "時間天氣", "正午", "time set noon", "", no.clone()),
        c("night", "時間天氣", "晚上", "time set night", "", no.clone()), c("midnight", "時間天氣", "午夜", "time set midnight", "", no.clone()),
        c("clear", "時間天氣", "放晴", "weather clear", "", no.clone()), c("rain", "時間天氣", "下雨", "weather rain", "", no.clone()),
        c("thunder", "時間天氣", "打雷", "weather thunder", "", no.clone()),
        c("time_stop", "時間天氣", "停在現在時間", "gamerule <rule> false", "", rule("doDaylightCycle")),
        c("time_go", "時間天氣", "恢復時間流動", "gamerule <rule> true", "", rule("doDaylightCycle")),
        c("weather_stop", "時間天氣", "停止天氣變化", "gamerule <rule> false", "", rule("doWeatherCycle")),
        c("weather_go", "時間天氣", "恢復天氣變化", "gamerule <rule> true", "", rule("doWeatherCycle")),
        c("gm_survival", "遊戲模式", "生存", "gamemode survival <player>", "", no.clone()),
        c("gm_creative", "遊戲模式", "創造", "gamemode creative <player>", "", no.clone()),
        c("gm_adventure", "遊戲模式", "冒險", "gamemode adventure <player>", "", no.clone()),
        c("gm_spectator", "遊戲模式", "旁觀", "gamemode spectator <player>", "", no.clone()),
        c("diff_peaceful", "遊戲模式", "難度：和平", "difficulty peaceful", "confirm", no.clone()),
        c("diff_easy", "遊戲模式", "難度：簡單", "difficulty easy", "", no.clone()),
        c("diff_normal", "遊戲模式", "難度：普通", "difficulty normal", "", no.clone()),
        c("diff_hard", "遊戲模式", "難度：困難", "difficulty hard", "", no.clone()),
        c("tp_player", "傳送", "傳到玩家旁邊", "tp <player> <target>", "", no.clone()),
        c("tp_xyz", "傳送", "傳到座標", "execute at <player> run tp <player> <x> <y> <z>", "", no.clone()),
        c("spawnpoint", "傳送", "把玩家重生點設在他現在的位置", "execute at <player> run spawnpoint <player> ~ ~ ~", "", no.clone()),
        c("worldspawn", "傳送", "把世界重生點設在玩家位置", "execute at <player> run setworldspawn ~ ~ ~", "", note("玩家要在主世界")),
        c("give", "給物品", "給物品", "give <player> minecraft:<item> <count>", "", no.clone()),
        c("enchant", "給物品", "附魔主手物品", "enchant <player> minecraft:<ench> <level>", "", note("手上要拿著能附魔的東西")),
        c("xp", "給物品", "給經驗等級", "xp add <player> <count> levels", "", no.clone()),
        c("summon", "召喚生物", "在玩家旁邊召喚", "execute at <player> run summon minecraft:<mob> ~ ~ ~", "", no.clone()),
        c("effect", "效果", "給效果", "effect give <player> minecraft:<effect> <secs> <amp> true", "", no.clone()),
        c("effect_clear", "效果", "清除效果", "effect clear <player>", "", no.clone()),
        c("keep_inv", "規則", "死亡不掉落", "gamerule <rule> <bool>", "", rule("keepInventory")),
        c("mob_spawn", "規則", "生物自然生成", "gamerule <rule> <bool>", "", json!({"rule": "doMobSpawning", "confirm_if_false": true})),
        c("mob_grief", "規則", "生物破壞方塊（苦力怕炸地形等）", "gamerule <rule> <bool>", "", json!({"rule": "mobGriefing", "confirm_if_false": true})),
        c("fire_tick", "規則", "火焰蔓延", "gamerule <rule> <bool>", "", rule("doFireTick")),
        c("sleep_pct", "規則", "幾成玩家睡覺就天亮", "gamerule <rule> <pct>", "", rule("playersSleepingPercentage")),
        c("insomnia", "規則", "幻翼出現", "gamerule <rule> <bool>", "", rule("doInsomnia")),
        c("instant_respawn", "規則", "死亡立即重生", "gamerule <rule> <bool>", "", rule("doImmediateRespawn")),
        c("list", "伺服器", "誰在線上", "list", "", no.clone()), c("save", "伺服器", "立刻存檔", "save-all", "", no.clone()),
        c("say", "伺服器", "廣播訊息", "say <msg>", "", no.clone()), c("seed", "伺服器", "查種子碼", "seed", "", no.clone()),
        c("wl_add", "伺服器", "加入白名單", "whitelist add <player>", "", no.clone()),
        c("wl_remove", "伺服器", "移出白名單", "whitelist remove <player>", "", no.clone()),
        c("wl_on", "伺服器", "開啟白名單", "whitelist on", "confirm", note("不在白名單的人會進不來")),
        c("wl_off", "伺服器", "關閉白名單", "whitelist off", "", no.clone()), c("wl_list", "伺服器", "看白名單", "whitelist list", "", no.clone()),
        c("kick", "伺服器", "踢出玩家", "kick <player> <reason>", "confirm", no.clone()),
        c("op", "伺服器", "給管理員", "op <player>", "strong", note("管理員能下所有指令")),
        c("deop", "伺服器", "拿掉管理員", "deop <player>", "", no.clone()),
        c("ban", "伺服器", "封鎖玩家", "ban <player>", "strong", no.clone()),
        c("pardon", "伺服器", "解除封鎖", "pardon <player>", "", no.clone()), c("banlist", "伺服器", "看封鎖名單", "banlist", "", no.clone()),
        c("kill_items", "清除", "清掉地上掉落物", "kill @e[type=minecraft:item]", "strong", note("地上所有掉落物會消失，不能復原")),
        c("kill_mob", "清除", "清掉某種生物", "kill @e[type=minecraft:<mob>]", "strong", note("這種生物全部消失（含有取名的），不能復原")),
        c("kill_hostile", "清除", "清掉所有敵對生物", "", "strong",
          json!({"special": "clear_hostile", "note": "做法是暫時切到和平再切回來；清不掉豬布林類、疣豬獸、終界龍"})),
        c("clear_inv", "清除", "清空玩家背包", "clear <player>", "strong", note("背包裡的東西全部消失，不能復原")),
    ]
}

fn danger_rank(d: &str) -> Option<u8> {
    match d {
        "" => Some(0),
        "confirm" => Some(1),
        "strong" => Some(2),
        _ => None,
    }
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

/// "1.21.1" → [1, 21, 1]；不是純數字版號回 None。
pub fn mc_tuple(v: &str) -> Option<Vec<u64>> {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"^\d+(\.\d+)+$"));
    RE.is_match(v).then(|| v.split('.').map(|x| x.parse().unwrap_or(u64::MAX)).collect())
}

/// 從清單上的版本字（如「1.21.1 · NeoForge」）取出遊戲版本；看不出來回 None。
pub fn mc_of(label: &str) -> Option<Vec<u64>> {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"\d+\.\d+(?:\.\d+)?"));
    RE.find(label).and_then(|m| mc_tuple(m.as_str()))
}

/// 回傳 (規則名, 是否不確定)；新版已移除回 (None, false)。
fn rule_name<'a>(old: &'a str, version: Option<&[u64]>) -> (Option<&'a str>, bool) {
    match version {
        None => (Some(old), true),
        Some(v) if v >= &RULE_RENAME_FROM[..] => (GAMERULE_NEW.iter().find(|r| r.0 == old).and_then(|r| r.1), false),
        Some(_) => (Some(old), false),
    }
}

fn tpl_fields(tpl: &str) -> Vec<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"<(\w+)>"));
    let mut out: Vec<String> = Vec::new();
    for c in RE.captures_iter(tpl) {
        if !out.iter().any(|f| f == &c[1]) {
            out.push(c[1].to_string());
        }
    }
    out
}

fn s<'a>(c: &'a Value, key: &str) -> &'a str {
    c.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Python 的真假值：None、False、0、空字串／清單／物件都算假。
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// 給畫面的清單：依伺服器版本換規則名、隱藏新版沒有的；版本看不出來時標 uncertain。
pub fn commands_for(label: &str, custom: &[Value]) -> Vec<Value> {
    let v = mc_of(label);
    let mut out = Vec::new();
    let custom = custom.iter().map(|c| {
        let mut c = c.clone();
        if !truthy(c.get("cat")) {
            c["cat"] = json!("我的筆記");
        }
        c
    });
    for mut c in builtin().into_iter().chain(custom) {
        let fields: Vec<String> = tpl_fields(s(&c, "tpl")).into_iter().filter(|f| f != "rule").collect();
        c["fields"] = json!(fields);
        if truthy(c.get("rule")) {
            let (name, unsure) = rule_name(s(&c, "rule"), v.as_deref());
            let Some(name) = name else { continue };
            c["preview"] = json!(s(&c, "tpl").replace("<rule>", name));
            c["uncertain"] = json!(unsure);
        }
        out.push(c);
    }
    out
}

fn check_value(key: &str, raw: Option<&Value>, ench: Option<&Value>) -> Result<String> {
    static PLAYER: LazyLock<Regex> = LazyLock::new(|| re(r"^\.?[A-Za-z0-9_]{1,16}$"));
    static ID: LazyLock<Regex> = LazyLock::new(|| re(r"^[a-z0-9_]{1,64}$"));
    static NUM: LazyLock<Regex> = LazyLock::new(|| re(r"^-?\d{1,8}(\.\d{1,3})?$"));
    let &(_, label, kind, optional) = field(key).expect("呼叫前已確認欄位存在");
    let raw = if matches!(raw, None | Some(Value::Null)) { String::new() } else { py_str(raw) };
    if raw.contains(['\r', '\n']) {
        // 先檢查再去空白：結尾的換行也要擋
        return Err(UserError::new(format!("{label}不能換行")));
    }
    let text = raw.trim();
    match kind {
        Kind::Player if text == "@a" || PLAYER.is_match(text) => Ok(text.into()),
        Kind::Player => Err(UserError::new(format!("{label}只能是 1–16 個英數字或底線（基岩版玩家前面可有一個句點），或 @a 代表全部"))),
        Kind::Id | Kind::Ench if !ID.is_match(text) || (key == "mob" && text == "player") => {
            Err(UserError::new(format!("{label}名稱只能是小寫英文、數字、底線（例如 diamond）")))
        }
        Kind::Id | Kind::Ench => Ok(text.into()),
        Kind::Int(min, max, _) => {
            // shortcut: 不接受 Python int() 才認的寫法（如 1_000、全形數字）, 比舊版嚴格不會放行壞值，有人回報再放寬
            let n: i128 = text.parse().map_err(|_| UserError::new(format!("{label}要填整數")))?;
            let top = if key == "level" {
                let e = ench.map(|e| py_str(Some(e)));
                ENCHANTS.iter().find(|x| Some(x.0) == e.as_deref()).map_or(max, |x| x.2)
            } else {
                max
            };
            if !(min as i128..=top as i128).contains(&n) {
                return Err(UserError::new(format!("{label}要在 {min} 到 {top} 之間")));
            }
            Ok((if key == "amp" { n - 1 } else { n }).to_string())
        }
        Kind::Num if NUM.is_match(text) => Ok(text.into()),
        Kind::Num => Err(UserError::new(format!("座標 {label} 要填數字"))),
        Kind::Bool => match text.to_lowercase().as_str() {
            b @ ("true" | "false") => Ok(b.into()),
            _ => Err(UserError::new(format!("{label}只能選開或關"))),
        },
        Kind::Text if text.is_empty() && !optional => Err(UserError::new(format!("{label}不能空白"))),
        Kind::Text if text.chars().count() > 200 => Err(UserError::new(format!("{label}最多 200 字"))),
        Kind::Text => Ok(text.into()),
    }
}

/// 畫面的「確認」：1.x 用 Python `in (True, "strong")` 比，所以數字 1 也算按過確認。
fn confirmed(confirm: &Value) -> bool {
    confirm == "strong" || confirm == &Value::Bool(true) || confirm.as_f64() == Some(1.0)
}

/// 回傳要送進主控台的字串；欄位不合法、危險指令沒確認、新版沒有的規則都回錯誤。
pub fn build_command(cmd: &Value, values: &Value, confirm: &Value, label: &str) -> Result<String> {
    let empty = Map::new();
    let values = values.as_object().unwrap_or(&empty);
    let mut out = s(cmd, "tpl").to_string();
    let mut danger = danger_rank(s(cmd, "danger")).unwrap_or(0);
    if truthy(cmd.get("rule")) {
        let Some(name) = rule_name(s(cmd, "rule"), mc_of(label).as_deref()).0 else {
            return Err(UserError::new(format!("「{}」這條規則在這個遊戲版本已經沒有了", s(cmd, "name"))));
        };
        out = out.replace("<rule>", name);
    }
    for key in tpl_fields(&out) {
        if field(&key).is_none() {
            return Err(UserError::new(format!("指令範本裡有不認識的欄位 <{key}>")));
        }
        let value = check_value(&key, values.get(&key), values.get("ench"))?;
        if value == "@a" || (truthy(cmd.get("confirm_if_false")) && value == "false") {
            danger = danger.max(1);
        }
        out = out.replace(&format!("<{key}>"), &value);
    }
    let out = out.trim().to_string();
    if danger == 2 && confirm != "strong" {
        return Err(UserError::new("這是會刪東西或給權限的指令，要先在畫面上強確認"));
    }
    if danger == 1 && !confirmed(confirm) {
        return Err(UserError::new("這個指令要先按確認"));
    }
    if out.is_empty() || out.contains(['\n', '\r']) {
        return Err(UserError::new("指令是空的或含有換行"));
    }
    Ok(out)
}

fn random_hex3() -> String {
    use std::hash::{BuildHasher, RandomState};
    format!("{:06x}", RandomState::new().hash_one(std::time::SystemTime::now()) & 0xFF_FFFF)
}

/// 我的筆記：驗證使用者自訂指令；沒有編號的給一個。
pub fn clean_custom_commands(items: &Value) -> Result<Vec<Value>> {
    static ID: LazyLock<Regex> = LazyLock::new(|| re(r"^my_[0-9a-z_]{1,20}$"));
    let items = items.as_array().filter(|a| a.len() <= 100).ok_or_else(|| UserError::new("我的筆記最多 100 筆"))?;
    let mut out = Vec::new();
    for c in items {
        let Some(o) = c.as_object() else { return Err(UserError::new("我的筆記格式不對")) };
        let get = |k: &str| py_str(Some(o.get(k).unwrap_or(&Value::String(String::new()))));
        let name = get("name").trim().to_string();
        let cat = if truthy(o.get("cat")) { get("cat") } else { "我的筆記".into() }.trim().to_string();
        let tpl = get("tpl").trim().trim_start_matches('/').to_string();
        let cid = if truthy(o.get("id")) { get("id") } else { format!("my_{}", random_hex3()) };
        let (nl, tl) = (name.chars().count(), tpl.chars().count());
        if !(1..=40).contains(&nl) || cat.chars().count() > 20 || !(1..=300).contains(&tl) || tpl.contains(['\n', '\r']) {
            let shown = if name.is_empty() { "（沒名稱）" } else { &name };
            return Err(UserError::new(format!("「{shown}」：名稱 1–40 字、指令 1–300 字且不能換行")));
        }
        if !ID.is_match(&cid) {
            return Err(UserError::new("我的筆記編號格式不對"));
        }
        if let Some(bad) = tpl_fields(&tpl).into_iter().find(|f| field(f).is_none() || f == "rule") {
            let all: Vec<String> = FIELDS.iter().map(|f| format!("<{}>", f.0)).collect();
            return Err(UserError::new(format!("「{name}」用了不認識的欄位 <{bad}>，可用：{}", all.join("、"))));
        }
        let danger = if truthy(o.get("danger")) { o.get("danger").and_then(Value::as_str) } else { Some("") };
        let Some(danger) = danger.filter(|d| danger_rank(d).is_some()) else {
            return Err(UserError::new("危險等級只能是無、確認、強確認"));
        };
        let note: String = get("note").chars().take(200).collect();
        out.push(json!({"id": cid, "cat": cat, "name": name, "tpl": tpl, "danger": danger, "note": note}));
    }
    Ok(out)
}

pub const DEFAULT_JOIN: &str = "【加入伺服器】\n1. 用 Minecraft Java 版（或 PrismLauncher）開 <版本>。\n\
2. 有裝模組的伺服器（<載入器>）：先裝同一個載入器，把伺服器給你的模組放進 mods 資料夾。\
Forge 和 NeoForge 的模組不能互通。\n3. 多人遊戲 → 新增伺服器 → 位址填：<連線網址>\n";

/// 「加入說明」：把範本裡的 <版本>、<載入器>、<連線網址> 換成這台的資料。
pub fn join_text(template: &str, server_cfg: &Value) -> String {
    let label = s(server_cfg, "version");
    // 1.0.0 加入的伺服器設定裡沒有 loader，從版本字樣（如「1.21.1 · Fabric · 100 模組」）推回來
    let guess = ["NeoForge", "Forge", "Fabric"].into_iter().find(|k| re(&format!(r"\b{k}\b")).is_match(label)).unwrap_or("");
    let loader = if truthy(server_cfg.get("loader")) {
        py_str(server_cfg.get("loader"))
    } else if !guess.is_empty() {
        guess.into()
    } else {
        "原版".into() // 1.x：版本字空白時這裡是空字串，但下面一樣換成「原版」
    };
    let address = if truthy(server_cfg.get("address")) { py_str(server_cfg.get("address")) } else { "（屋主還沒設定連線網址）".into() };
    let version = mc_of(label).map_or("伺服器指定的版本".into(), |v| v.iter().map(u64::to_string).collect::<Vec<_>>().join("."));
    template.replace("<連線網址>", &address).replace("<版本>", &version).replace("<載入器>", &loader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_shape() {
        let all = builtin();
        assert_eq!(all.len(), 55);
        let mut ids: Vec<&str> = all.iter().map(|c| s(c, "id")).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 55, "指令編號不能重複");
        for c in &all {
            assert!(danger_rank(s(c, "danger")).is_some());
            assert!(tpl_fields(s(c, "tpl")).iter().all(|f| f == "rule" || field(f).is_some()), "{c}");
        }
        assert_eq!(fields_json().as_object().unwrap().len(), 17);
        assert_eq!(fields_json()["count"]["max"], 640);
    }

    #[test]
    fn custom_without_id_gets_one() {
        let out = clean_custom_commands(&json!([{"name": "回家", "tpl": "/tp <player> 0 64 0"}])).unwrap();
        assert!(ID_OK.is_match(s(&out[0], "id")), "{}", out[0]);
        assert_eq!(out[0]["tpl"], "tp <player> 0 64 0");
    }
    static ID_OK: LazyLock<Regex> = LazyLock::new(|| re(r"^my_[0-9a-f]{6}$"));

    #[test]
    fn confirm_number_one_counts() {
        let cmd = &builtin()[15]; // diff_peaceful
        assert_eq!(build_command(cmd, &json!({}), &json!(1), "1.21.1").unwrap(), "difficulty peaceful");
        assert!(build_command(cmd, &json!({}), &json!(false), "1.21.1").is_err());
    }
}
