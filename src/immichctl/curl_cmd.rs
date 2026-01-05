use anyhow::Result;
use progenitor_client::{ClientHooks, ClientInfo, OperationInfo};

use super::Client;
use super::ImmichCtl;

/// Supported HTTP methods for the curl command.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum CurlMethod {
    #[value(alias("GET"))]
    Get,
    #[value(alias("POST"))]
    Post,
    #[value(alias("PUT"))]
    Put,
    #[value(alias("DELETE"))]
    Delete,
}

impl ImmichCtl {
    pub async fn curl(&self, path: &str, method: CurlMethod, data: &Option<String>) -> Result<()> {
        self.assert_logged_in()?;

        match method {
            CurlMethod::Get => self.curl_get(path).await,
            CurlMethod::Post => self.curl_post(path, data).await,
            CurlMethod::Put => self.curl_put(path, data).await,
            CurlMethod::Delete => self.curl_delete(path, data).await,
        }
    }

    async fn curl_get(&self, path: &str) -> Result<()> {
        let immich = self.immich()?;
        let url = format!("{}/{}", immich.baseurl, path);
        let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
        header_map.append(
            ::reqwest::header::HeaderName::from_static("api-version"),
            ::reqwest::header::HeaderValue::from_static(Client::api_version()),
        );

        let request = immich
            .client
            .get(url)
            .header(
                ::reqwest::header::ACCEPT,
                ::reqwest::header::HeaderValue::from_static("application/json"),
            )
            .headers(header_map)
            .build()?;
        self.exec_request(request).await
    }

    async fn curl_post(&self, path: &str, data: &Option<String>) -> Result<()> {
        let immich = self.immich()?;
        let url = format!("{}/{}", immich.baseurl, path);
        let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
        header_map.append(
            ::reqwest::header::HeaderName::from_static("api-version"),
            ::reqwest::header::HeaderValue::from_static(Client::api_version()),
        );

        let mut request_builder = immich
            .client
            .post(url)
            .header(
                ::reqwest::header::ACCEPT,
                ::reqwest::header::HeaderValue::from_static("application/json"),
            )
            .headers(header_map);

        if let Some(json) = Self::parse_data_to_json(data) {
            request_builder = request_builder.json(&json);
        }

        let request = request_builder.build()?;
        self.exec_request(request).await
    }

    async fn curl_put(&self, path: &str, data: &Option<String>) -> Result<()> {
        let immich = self.immich()?;
        let url = format!("{}/{}", immich.baseurl, path);
        let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
        header_map.append(
            ::reqwest::header::HeaderName::from_static("api-version"),
            ::reqwest::header::HeaderValue::from_static(Client::api_version()),
        );

        let mut request_builder = immich
            .client
            .put(url)
            .header(
                ::reqwest::header::ACCEPT,
                ::reqwest::header::HeaderValue::from_static("application/json"),
            )
            .headers(header_map);

        if let Some(json) = Self::parse_data_to_json(data) {
            request_builder = request_builder.json(&json);
        }

        let request = request_builder.build()?;
        self.exec_request(request).await
    }

    async fn curl_delete(&self, path: &str, data: &Option<String>) -> Result<()> {
        let immich = self.immich()?;
        let url = format!("{}/{}", immich.baseurl, path);
        let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
        header_map.append(
            ::reqwest::header::HeaderName::from_static("api-version"),
            ::reqwest::header::HeaderValue::from_static(Client::api_version()),
        );

        let mut request_builder = immich
            .client
            .delete(url)
            .header(
                ::reqwest::header::ACCEPT,
                ::reqwest::header::HeaderValue::from_static("application/json"),
            )
            .headers(header_map);

        if let Some(json) = Self::parse_data_to_json(data) {
            request_builder = request_builder.json(&json);
        }

        let request = request_builder.build()?;
        self.exec_request(request).await
    }

    async fn exec_request(&self, request: reqwest::Request) -> Result<()> {
        let immich = self.immich()?;
        let info = OperationInfo {
            operation_id: "curl",
        };
        immich
            .pre::<progenitor_client::Error>(&mut request.try_clone().unwrap(), &info)
            .await?;
        let result = immich.exec(request, &info).await;
        immich
            .post::<progenitor_client::Error>(&result, &info)
            .await?;
        let response = result?;
        match response.status().as_u16() {
            200u16..300u16 => {
                let body = response.bytes().await?.to_vec();
                // Print response body as formatted JSON if possible
                match serde_json::from_slice::<serde_json::Value>(&body) {
                    Ok(json) => {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json)
                                .unwrap_or_else(|_| String::from_utf8_lossy(&body).to_string())
                        );
                    }
                    Err(_) => {
                        // Fallback: print as plain text
                        println!("{}", String::from_utf8_lossy(&body));
                    }
                }
                Ok(())
            }
            _ => Err(
                progenitor_client::Error::<progenitor_client::Error>::UnexpectedResponse(response)
                    .into(),
            ),
        }
    }

    /// Parse `--data` into a JSON value.
    ///
    /// Accepts three forms:
    /// - Valid JSON → used as-is
    /// - `key=value` or `a=b&c=d` → converted to JSON object of strings
    /// - Any other string → JSON string
    fn parse_data_to_json(data: &Option<String>) -> Option<serde_json::Value> {
        match data {
            Some(body) => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                    return Some(json);
                }
                let mut map = serde_json::Map::new();
                for pair in body.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
                    }
                }
                if map.is_empty() {
                    Some(serde_json::Value::String(body.to_string()))
                } else {
                    Some(serde_json::Value::Object(map))
                }
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod curl_cmd_tests {
    use super::*;

    #[test]
    fn parse_json_object() {
        let data = Some("{\"id\":\"abc\"}".to_string());
        let v = ImmichCtl::parse_data_to_json(&data).unwrap();
        assert_eq!(
            v.get("id").unwrap(),
            &serde_json::Value::String("abc".to_string())
        );
    }

    #[test]
    fn parse_kv_pairs_to_object() {
        let data = Some("id=abc&x=1".to_string());
        let v = ImmichCtl::parse_data_to_json(&data).unwrap();
        assert_eq!(
            v.get("id").unwrap(),
            &serde_json::Value::String("abc".to_string())
        );
        assert_eq!(
            v.get("x").unwrap(),
            &serde_json::Value::String("1".to_string())
        );
    }

    #[test]
    fn parse_fallback_string() {
        let data = Some("hello".to_string());
        let v = ImmichCtl::parse_data_to_json(&data).unwrap();
        assert_eq!(v, serde_json::Value::String("hello".to_string()));
    }

    #[test]
    fn parse_none() {
        let data: Option<String> = None;
        assert!(ImmichCtl::parse_data_to_json(&data).is_none());
    }
}
