#[derive(Clone)]
pub(crate) struct SimpleConfigForm {
    pub(crate) server_addr: String,
    pub(crate) server_port: String,
    pub(crate) auth_token: String,
    pub(crate) proxy_name: String,
    pub(crate) proxy_type: String,
    pub(crate) local_ip: String,
    pub(crate) local_port: String,
    pub(crate) remote_port: String,
}

impl Default for SimpleConfigForm {
    fn default() -> Self {
        Self {
            server_addr: String::new(),
            server_port: "7000".to_string(),
            auth_token: String::new(),
            proxy_name: "minecraft".to_string(),
            proxy_type: "tcp".to_string(),
            local_ip: "127.0.0.1".to_string(),
            local_port: "25565".to_string(),
            remote_port: "25565".to_string(),
        }
    }
}

impl SimpleConfigForm {
    pub(crate) fn apply_from_toml(&mut self, text: &str) {
        let Ok(v) = toml::from_str::<toml::Value>(text) else {
            return;
        };

        if let Some(s) = v.get("serverAddr").and_then(|x| x.as_str()) {
            self.server_addr = s.to_string();
        }
        if let Some(n) = v.get("serverPort").and_then(|x| x.as_integer()) {
            self.server_port = n.to_string();
        }

        if let Some(token) = v
            .get("auth")
            .and_then(|x| x.get("token"))
            .and_then(|x| x.as_str())
        {
            self.auth_token = token.to_string();
        }

        if let Some(proxy) = v
            .get("proxies")
            .and_then(|x| x.as_array())
            .and_then(|arr| arr.first())
        {
            if let Some(name) = proxy.get("name").and_then(|x| x.as_str()) {
                self.proxy_name = name.to_string();
            }
            if let Some(ty) = proxy.get("type").and_then(|x| x.as_str()) {
                self.proxy_type = normalize_proxy_type(ty);
            }
            if let Some(ip) = proxy.get("localIP").and_then(|x| x.as_str()) {
                self.local_ip = ip.to_string();
            }
            if let Some(p) = proxy.get("localPort").and_then(|x| x.as_integer()) {
                self.local_port = p.to_string();
            }
            if let Some(p) = proxy.get("remotePort").and_then(|x| x.as_integer()) {
                self.remote_port = p.to_string();
            }
        }
    }

    pub(crate) fn to_toml(&self) -> String {
        let server_port = self.server_port.trim().parse::<u16>().unwrap_or(7000);
        let local_port = self.local_port.trim().parse::<u16>().unwrap_or(0);
        let remote_port = self.remote_port.trim().parse::<u16>().unwrap_or(0);

        let mut out = String::new();
        out.push_str(&format!(
            "serverAddr = \"{}\"\n",
            escape_toml_string(&self.server_addr)
        ));
        out.push_str(&format!("serverPort = {}\n", server_port));

        if !self.auth_token.trim().is_empty() {
            out.push('\n');
            out.push_str("auth.method = \"token\"\n");
            out.push_str(&format!(
                "auth.token = \"{}\"\n",
                escape_toml_string(self.auth_token.trim())
            ));
        }

        out.push('\n');
        out.push_str("[[proxies]]\n");
        out.push_str(&format!(
            "name = \"{}\"\n",
            escape_toml_string(&self.proxy_name)
        ));
        out.push_str(&format!(
            "type = \"{}\"\n",
            escape_toml_string(&self.proxy_type)
        ));
        out.push_str(&format!(
            "localIP = \"{}\"\n",
            escape_toml_string(&self.local_ip)
        ));
        out.push_str(&format!("localPort = {}\n", local_port));
        out.push_str(&format!("remotePort = {}\n", remote_port));
        out
    }
}

pub(crate) fn normalize_proxy_type(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "udp" => "udp".to_string(),
        _ => "tcp".to_string(),
    }
}

pub(crate) fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\"', "\\\"")
}
