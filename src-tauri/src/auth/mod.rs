use crate::config::{self, Account, AccountsFile};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

/// Mojang offline UUID: UUID.nameUUIDFromBytes("OfflinePlayer:"+name) — MD5, version 3.
pub fn offline_uuid(username: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{username}").as_bytes());
    let mut h = hasher.finalize();
    h[6] = (h[6] & 0x0f) | 0x30; // version 3
    h[8] = (h[8] & 0x3f) | 0x80; // IETF variant
    let hex = hex::encode(h);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn offline_login(username: &str) -> Account {
    let name = {
        let t = username.trim();
        if t.is_empty() {
            "Player".to_string()
        } else {
            t.to_string()
        }
    };
    Account {
        mode: "offline".into(),
        username: name.clone(),
        uuid: offline_uuid(&name),
        access_token: "0".into(),
        client_token: "0".into(),
        user_type: "legacy".into(),
        authlib_api: String::new(),
    }
}

#[derive(Serialize)]
struct AuthAgent {
    name: String,
    version: i32,
}

#[derive(Serialize)]
struct AuthenticateRequest {
    agent: AuthAgent,
    username: String,
    password: String,
    #[serde(rename = "clientToken", skip_serializing_if = "Option::is_none")]
    client_token: Option<String>,
    #[serde(rename = "requestUser")]
    request_user: bool,
}

#[derive(Deserialize)]
struct Profile {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct AuthenticateResponse {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "clientToken")]
    client_token: Option<String>,
    #[serde(rename = "selectedProfile")]
    selected_profile: Option<Profile>,
    #[serde(rename = "availableProfiles")]
    available_profiles: Option<Vec<Profile>>,
    error: Option<String>,
    #[serde(rename = "errorMessage")]
    error_message: Option<String>,
}

fn format_uuid(id: &str) -> String {
    let id: String = id.chars().filter(|c| *c != '-').collect();
    if id.len() != 32 {
        return id;
    }
    format!(
        "{}-{}-{}-{}-{}",
        &id[0..8],
        &id[8..12],
        &id[12..16],
        &id[16..20],
        &id[20..32]
    )
}

pub async fn yggdrasil_login(
    api_root: &str,
    username: &str,
    password: &str,
) -> Result<Account, String> {
    let root = api_root.trim().trim_end_matches('/').to_string();
    if root.is_empty() {
        return Err("Authlib Injector API 地址为空".into());
    }
    if username.trim().is_empty() || password.is_empty() {
        return Err("用户名或密码为空".into());
    }
    let url = format!("{root}/authserver/authenticate");
    let body = AuthenticateRequest {
        agent: AuthAgent {
            name: "Minecraft".into(),
            version: 1,
        },
        username: username.trim().to_string(),
        password: password.to_string(),
        client_token: None,
        request_user: true,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("mclauncher-rust1/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("网络错误: {e}"))?;
    let status = resp.status();
    let data: AuthenticateResponse = resp
        .json()
        .await
        .map_err(|e| format!("解析认证响应失败: {e} (HTTP {status})"))?;
    if data.error.as_deref().unwrap_or("").len() > 0
        || data.error_message.as_deref().unwrap_or("").len() > 0
    {
        let msg = data
            .error_message
            .or(data.error)
            .unwrap_or_else(|| "未知错误".into());
        return Err(format!("认证失败: {msg}"));
    }
    if status.is_client_error() || status.is_server_error() {
        return Err(format!("认证失败: HTTP {status}"));
    }
    let access = data
        .access_token
        .ok_or_else(|| "认证响应缺少 accessToken".to_string())?;
    let prof = data
        .selected_profile
        .or_else(|| data.available_profiles.and_then(|v| v.into_iter().next()))
        .ok_or_else(|| "认证成功但无可用角色".to_string())?;
    Ok(Account {
        mode: "authlib".into(),
        username: prof.name,
        uuid: format_uuid(&prof.id),
        access_token: access,
        client_token: data.client_token.unwrap_or_default(),
        user_type: "mojang".into(),
        authlib_api: root,
    })
}

pub fn persist_current(account: Account) -> Result<Account, String> {
    let mut file = config::load_accounts();
    // Replace same username+mode or push
    if let Some(pos) = file
        .accounts
        .iter()
        .position(|a| {
            a.username == account.username
                && a.mode == account.mode
                && a.authlib_api == account.authlib_api
        })
    {
        file.accounts[pos] = account.clone();
    } else {
        file.accounts.push(account.clone());
    }
    file.current = Some(account.clone());
    config::save_accounts(&file)?;
    Ok(account)
}

pub fn logout() -> Result<(), String> {
    let mut file = config::load_accounts();
    file.current = None;
    config::save_accounts(&file)
}

pub fn list() -> AccountsFile {
    config::load_accounts()
}

/// 切换到已保存的第 index 个账户。
pub fn switch_to(file: &mut AccountsFile, index: usize) -> Result<Account, String> {
    let a = file
        .accounts
        .get(index)
        .cloned()
        .ok_or_else(|| "账户不存在".to_string())?;
    file.current = Some(a.clone());
    Ok(a)
}

/// 删除已保存的第 index 个账户；若删的是当前账户则同时退出登录。
pub fn remove_at(file: &mut AccountsFile, index: usize) -> Result<(), String> {
    if index >= file.accounts.len() {
        return Err("账户不存在".into());
    }
    let removed = file.accounts.remove(index);
    if file
        .current
        .as_ref()
        .map(|c| c.username == removed.username && c.mode == removed.mode && c.authlib_api == removed.authlib_api)
        .unwrap_or(false)
    {
        file.current = None;
    }
    Ok(())
}

pub fn switch_account(index: usize) -> Result<Account, String> {
    let mut file = config::load_accounts();
    let a = switch_to(&mut file, index)?;
    config::save_accounts(&file)?;
    Ok(a)
}

pub fn remove_account(index: usize) -> Result<AccountsFile, String> {
    let mut file = config::load_accounts();
    remove_at(&mut file, index)?;
    config::save_accounts(&file)?;
    Ok(file)
}

/// 微软正版登录：设置 → 账户 中按钮显示「暂未开放」，此处保留占位。
#[allow(dead_code)]
pub fn microsoft_login_stub() -> Result<Account, String> {
    Err("微软登录暂未开放，请使用离线或 authlib-injector 外置登录".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_mojang() {
        assert_eq!(offline_uuid("Steve"), "5627dd98-e6be-3c21-b8a8-e92344183641");
    }

    #[test]
    fn offline_login_defaults_player() {
        let a = offline_login("   ");
        assert_eq!(a.username, "Player");
        assert_eq!(a.mode, "offline");
    }

    #[test]
    fn switch_and_remove() {
        let mut f = AccountsFile::default();
        f.accounts.push(offline_login("A"));
        f.accounts.push(offline_login("B"));
        let b = switch_to(&mut f, 1).unwrap();
        assert_eq!(b.username, "B");
        assert_eq!(f.current.as_ref().unwrap().username, "B");
        remove_at(&mut f, 1).unwrap();
        assert!(f.current.is_none());
        assert_eq!(f.accounts.len(), 1);
        assert!(switch_to(&mut f, 5).is_err());
    }
}
