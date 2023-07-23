use std::env;

use clap::command;
use clap::Parser;
use dotenv::dotenv;
use log::error;
use reqwest::header;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use serde_json::Value;

use log::{info, warn};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Record name
    #[arg(short, long)]
    name: String,

    /// Zone Id
    #[arg(short, long)]
    zone: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(rename = "Root")]
pub struct Response {
    #[serde(rename = "result")]
    pub records: Vec<Record>,
    pub success: bool,
    pub errors: Vec<Value>,
    pub messages: Vec<Value>,
    #[serde(rename = "result_info")]
    pub result_info: ResultInfo,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(rename = "result")]
pub struct Record {
    pub id: String,
    #[serde(rename = "zone_id")]
    pub zone_id: String,
    #[serde(rename = "zone_name")]
    pub zone_name: String,
    pub name: String,
    #[serde(rename = "type")]
    pub type_field: String,
    #[serde(rename = "content")]
    pub ip_addr: String,
    pub proxiable: bool,
    pub proxied: bool,
    pub ttl: i64,
    pub locked: bool,
    pub meta: Meta,
    pub comment: Option<String>,
    pub tags: Vec<Value>,
    #[serde(rename = "created_on")]
    pub created_on: String,
    #[serde(rename = "modified_on")]
    pub modified_on: String,
    pub priority: Option<i64>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(rename = "auto_added")]
    pub auto_added: bool,
    #[serde(rename = "managed_by_apps")]
    pub managed_by_apps: bool,
    #[serde(rename = "managed_by_argo_tunnel")]
    pub managed_by_argo_tunnel: bool,
    pub source: String,
    #[serde(rename = "email_routing")]
    pub email_routing: Option<bool>,
    #[serde(rename = "read_only")]
    pub read_only: Option<bool>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultInfo {
    pub page: i64,
    #[serde(rename = "per_page")]
    pub per_page: i64,
    pub count: i64,
    #[serde(rename = "total_count")]
    pub total_count: i64,
    #[serde(rename = "total_pages")]
    pub total_pages: i64,
}

fn find_subdomain_record<'a>(records: &'a [Record], record_name: &'a String) -> Option<&'a Record> {
    records
        .iter()
        .find(|record| record.name == *record_name && record.type_field == "A")
}

fn get_dns_records(client: &reqwest::blocking::Client, zone: &String) -> String {
    let dotoken = env::var("CF_TOKEN").expect("No CF_TOKEN set in env");
    let mut headers = header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert(
        "Authorization",
        format!("Bearer {}", dotoken).parse().unwrap(),
    );

    client
        .get(format!(
            "https://api.cloudflare.com/client/v4/zones/{zone}/dns_records"
        ))
        .headers(headers)
        .send()
        .expect("could not send request to cloudflare api")
        .text()
        .expect("could not retrieve text from cloudflare api response")
}

fn get_current_ip_addr(client: &reqwest::blocking::Client) -> String {
    client
        .get("http://whatismyip.akamai.com/")
        .send()
        .expect("could not send request to whatismyip api")
        .text()
        .expect("could not retrieve text from whatismyip api response")
}

fn update_record(
    client: &reqwest::blocking::Client,
    current_ip: &String,
    record: &Record,
    zone: &String,
) {
    let dotoken = env::var("CF_TOKEN").expect("No CF_TOKEN set in env");
    let mut headers = header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert(
        "Authorization",
        format!("Bearer {}", dotoken).parse().unwrap(),
    );

    let record_id = record.id.clone();
    let mut new_record = record.clone();
    new_record.ip_addr = String::from(current_ip);
    let payload = serde_json::to_string_pretty(&new_record)
        .map_err(|e| error!("Failed to construct updated record payload {}", e))
        .unwrap();

    let response = client
        .put(format!(
            "https://api.cloudflare.com/client/v4/zones/{zone}/dns_records/{record_id}"
        ))
        .headers(headers)
        .body(payload)
        .send()
        .expect("could not send request to cloudflare api")
        .text()
        .expect("could not get respose body");
    if response.contains("success\":true") {
        info!("Successfully updated DNS record")
    } else {
        error!("Record Update Failed, DNS not synced with actual ip!")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();
    let args = Args::parse();
    if args.name.is_empty() {
        error!("Record domain can not be empty!");
        panic!();
    }
    if args.zone.is_empty() {
        error!("Zone can not be empty!");
        panic!();
    }

    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let res: Response = serde_json::from_str(get_dns_records(&client, &args.zone).as_str())
        .expect("Could not parse Cloudflare response JSON");
    info!(
        "Fetched All DNS records from Cloudflare for zone {}",
        args.zone
    );

    let current_ip = get_current_ip_addr(&client);

    match find_subdomain_record(&res.records, &args.name) {
        Some(record) => match current_ip == record.ip_addr {
            true => update_record(&client, &current_ip, record, &args.zone),
            false => info!("Nothing to update, DNS in sync"),
        },
        None => {
            warn!("No record for subdomain {} found ", &args.name);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::get_current_ip_addr;

    struct Setup {
        client: reqwest::blocking::Client,
    }

    impl Setup {
        fn new() -> Self {
            Self {
                client: reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
            }
        }
    }
    #[test]
    fn test_current_ip() {
        let setup = Setup::new();
        let current_ip = get_current_ip_addr(&setup.client);
        assert_ne!(current_ip, "0.0.0.0");
    }
}
