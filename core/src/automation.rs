//! 可控時鐘的重開狀態機；不執行程序、不接觸 Tauri。
use crate::{Result, UserError};
use serde_json::{Value, json};
#[derive(Clone, Debug)]
pub struct Policy {
    pub crash_enabled: bool,
    pub crash_limit: u32,
    pub crash_delay: u64,
    pub schedule_enabled: bool,
    pub schedule_time: String,
    pub countdown: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            crash_enabled: false,
            crash_limit: 3,
            crash_delay: 30,
            schedule_enabled: false,
            schedule_time: "04:00".into(),
            countdown: 60,
        }
    }
}
impl Policy {
    pub fn parse(v: &Value) -> Result<Self> {
        let mut p = Self::default();
        if v.is_null() {
            return Ok(p);
        }
        let obj = v
            .as_object()
            .ok_or_else(|| UserError::new("重開設定格式不正確"))?;
        for (k, v) in obj {
            match k.as_str() {
                "crash_enabled" => {
                    p.crash_enabled = v
                        .as_bool()
                        .ok_or_else(|| UserError::new("請選擇是否當機重開"))?
                }
                "schedule_enabled" => {
                    p.schedule_enabled = v
                        .as_bool()
                        .ok_or_else(|| UserError::new("請選擇是否排程重開"))?
                }
                "crash_limit" => {
                    p.crash_limit = v
                        .as_u64()
                        .filter(|n| (1..=10).contains(n))
                        .ok_or_else(|| UserError::new("當機重開上限須為 1–10 次"))?
                        as u32
                }
                "crash_delay" => {
                    p.crash_delay = v
                        .as_u64()
                        .filter(|n| (5..=600).contains(n))
                        .ok_or_else(|| UserError::new("當機後等待須為 5–600 秒"))?
                }
                "countdown" => {
                    p.countdown = v
                        .as_u64()
                        .filter(|n| (10..=600).contains(n))
                        .ok_or_else(|| UserError::new("重開前倒數須為 10–600 秒"))?
                }
                "schedule_time" => {
                    p.schedule_time = v
                        .as_str()
                        .filter(|s| crate::settings::valid_backup_time(s))
                        .ok_or_else(|| UserError::new("重開時間須為 HH:MM"))?
                        .into()
                }
                _ => return Err(UserError::new("不認識這項重開設定")),
            }
        }
        Ok(p)
    }
    pub fn to_json(&self) -> Value {
        json!({"crash_enabled":self.crash_enabled,"crash_limit":self.crash_limit,"crash_delay":self.crash_delay,"schedule_enabled":self.schedule_enabled,"schedule_time":self.schedule_time,"countdown":self.countdown})
    }
}
#[derive(Debug, PartialEq)]
pub enum Action {
    Broadcast(u64),
    Stop,
    Start,
}
#[derive(Default, Debug)]
pub struct Runtime {
    pub attempts: u32,
    due: Option<u64>,
    due_crash: bool,
    countdown: Option<u64>,
    bucket: u64,
    waiting_stop: bool,
    manual: bool,
    pub note: String,
}
impl Runtime {
    pub fn manual_start(&mut self) {
        *self = Self::default();
    }
    pub fn cancel(&mut self) {
        self.due = None;
        self.countdown = None;
        self.waiting_stop = false;
        self.manual = true;
        self.note = "已取消待執行重開".into();
    }
    pub fn exited(&mut self, p: &Policy, code: i64, now: u64) {
        if self.waiting_stop {
            self.waiting_stop = false;
            self.due_crash = false;
            if code == 0 {
                self.due = Some(now);
                self.note = "排程已正常關機，等待備份完成再重開".into();
            } else {
                self.due = None;
                self.note = "排程關機異常，已取消重開，請人工檢查".into();
            }
            return;
        }
        self.countdown = None;
        if !self.manual && code != 0 && p.crash_enabled && self.attempts < p.crash_limit {
            self.due_crash = true;
            self.due = Some(now + p.crash_delay * 1000);
            self.note = format!(
                "當機後等待 {} 秒，第 {}／{} 次自動重開",
                p.crash_delay,
                self.attempts + 1,
                p.crash_limit
            );
        } else {
            self.due = None;
            if code != 0 && p.crash_enabled && !self.manual {
                self.note = "已達當機重開上限，請人工檢查；手動開機會重設次數".into();
            }
        }
    }
    pub fn schedule(&mut self, p: &Policy, now: u64, running: bool) -> Vec<Action> {
        if !p.schedule_enabled
            || !running
            || self.countdown.is_some()
            || self.waiting_stop
            || self.due.is_some()
        {
            self.note = "今天的排程未執行：伺服器未就緒或已有重開工作".into();
            return vec![];
        }
        self.manual = false;
        self.countdown = Some(now + p.countdown * 1000);
        self.bucket = if p.countdown > 30 {
            3
        } else if p.countdown > 10 {
            2
        } else {
            1
        };
        self.note = format!("排程重開倒數 {} 秒", p.countdown);
        vec![Action::Broadcast(p.countdown)]
    }
    pub fn tick(&mut self, p: &Policy, now: u64, running: bool, can_start: bool) -> Vec<Action> {
        if self.due.is_some()
            && ((self.due_crash && !p.crash_enabled) || (!self.due_crash && !p.schedule_enabled))
        {
            self.cancel();
            return vec![];
        }
        if let Some(end) = self.countdown {
            if !p.schedule_enabled || !running {
                self.countdown = None;
                self.note = "倒數已取消：排程關閉或伺服器不在執行中".into();
                return vec![];
            }
            if now >= end {
                self.countdown = None;
                self.waiting_stop = true;
                self.note = "已倒數完畢，等待正常存檔關機".into();
                return vec![Action::Stop];
            }
            let left = (end - now).div_ceil(1000);
            let bucket = if left > 30 {
                3
            } else if left > 10 {
                2
            } else {
                1
            };
            if bucket < self.bucket {
                self.bucket = bucket;
                return vec![Action::Broadcast(left)];
            }
        }
        if self.due.is_some_and(|t| now >= t) && !running && can_start {
            self.due = None;
            if self.due_crash {
                self.attempts += 1;
            }
            return vec![Action::Start];
        }
        vec![]
    }
    pub fn pending(&self) -> bool {
        self.due.is_some() || self.countdown.is_some() || self.waiting_stop
    }
    pub fn start_failed(&mut self, msg: &str) {
        self.due = None;
        self.waiting_stop = false;
        self.note = format!("自動重開被擋，請人工檢查：{msg}");
    }
}
