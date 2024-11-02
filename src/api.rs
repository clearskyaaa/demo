use crate::my_window;
use anyhow::Result;
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::{future, pin_mut, Stream, StreamExt};
use lazy_static::lazy_static;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::HashMap;
use std::os::raw::c_void;
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{client_async_tls, connect_async_tls_with_config, WebSocketStream};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum FlexibleValue {
    Array(Vec<Value>),
    Object(serde_json::Map<String, Value>),
    String(String),
    Int(i32),
    Bool(bool),
}

#[derive(Debug, Deserialize)]
struct ApiResult {
    result: Option<FlexibleValue>,
    id: u32,
}

fn string_to_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse::<f64>().map_err(serde::de::Error::custom)
}

#[derive(Debug, Deserialize)]
pub struct Price {
    #[serde(rename = "e")]
    pub event_type: String,
    #[serde(rename = "E")]
    pub time_stamp: u64,
    #[serde(rename = "s")]
    pub name: String,
    #[serde(rename = "p", deserialize_with = "string_to_f64")]
    pub tag_price: f64,
    #[serde(rename = "i", deserialize_with = "string_to_f64")]
    pub spot_index_price: f64,
    #[serde(rename = "P", deserialize_with = "string_to_f64")]
    pub predict_price: f64,
    #[serde(rename = "r", deserialize_with = "string_to_f64")]
    pub fee: f64,
    #[serde(rename = "T")]
    pub next_fee_time: u64,
}

pub enum ApiMessage {
    Price(Price),
    Notify(String),
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum TradePair {
    BTCUSDT,
    ETHUSDT,
    SOLUSDT,
}
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct TradePairInfo {
    pub ws_name: String,
    pub show_name: String,
    pub pair_name: String,
}

lazy_static! {
    pub static ref TRADE_INFO: HashMap<TradePair, TradePairInfo> = [
        (
            TradePair::BTCUSDT,
            TradePairInfo {
                ws_name: "btcusdt@markPrice".to_string(),
                show_name: "BTC/USDT".to_string(),
                pair_name: "BTCUSDT".to_string(),
            }
        ),
        (
            TradePair::ETHUSDT,
            TradePairInfo {
                ws_name: "ethusdt@markPrice".to_string(),
                show_name: "ETH/USDT".to_string(),
                pair_name: "ETHUSDT".to_string()
            }
        ),
        (
            TradePair::SOLUSDT,
            TradePairInfo {
                ws_name: "solusdt@markPrice".to_string(),
                show_name: "SOL/USDT".to_string(),
                pair_name: "SOLUSDT".to_string()
            }
        ),
    ]
    .iter()
    .cloned()
    .collect();
}

fn send_message_to_ui(hwnd: usize, message: ApiMessage) {
    let message_p = Box::into_raw(Box::new(message)) as *mut c_void;
    unsafe {
        let _ = PostMessageW(
            HWND(hwnd as *mut c_void),
            my_window::Window::WM_FRESH,
            WPARAM(message_p as usize),
            LPARAM::default(),
        )
        .expect("post message error");
    }
}

use tokio::time::{self, Duration};
async fn ws_handle<T>(
    ws_stream: T,
    trade_pair_arc: Arc<Mutex<TradePair>>,
    hwnd: usize,
    tx: UnboundedSender<Message>,
    rx: &mut UnboundedReceiver<Message>,
) where
    T: Stream<
        Item = Result<
            tokio_tungstenite::tungstenite::Message,
            tokio_tungstenite::tungstenite::Error,
        >,
    >,
    T: futures_util::Sink<Message> + Unpin,
{
    {
        let trade_pair = trade_pair_arc.lock().unwrap();
        subscribe(&trade_pair, tx.clone());
    }
    let (write, mut read) = ws_stream.split();
    let send_to_ws = rx.map(Ok).forward(write);
    let timeout_duration = Duration::from_secs(10); 
    let receiv_from_ws = async{
        loop{
            let timeout_result = time::timeout(timeout_duration, read.next()).await;
            if timeout_result.is_err(){
                println!("连接超时");
                let test_msg = Message::Text("haha".to_string());
                    tx.unbounded_send(test_msg).unwrap();
                continue;
            }
            let result = timeout_result.unwrap();
            if result.is_none(){
                break;
            }
            let message =result.unwrap();
            match message {
                Ok(Message::Text(str_data)) => {
                    println!("str_data:{}", str_data);
                    let price = serde_json::from_str::<Price>(&str_data);
                    if !price.is_ok() {
                        // let api_result = serde_json::from_str::<ApiResult>(&str_data);
                        // if !api_result.is_ok() {
                        //     break;
                        // }
                        // continue;
                        continue;
                    }
                    let price = price.unwrap();
                    send_message_to_ui(hwnd, ApiMessage::Price(price));
                }
                Ok(Message::Ping(payload)) => {
                    println!("ping");
                    let pong_msg = Message::Pong(payload.clone());
                    tx.unbounded_send(pong_msg).unwrap();
                }
                Ok(Message::Close(_)) => {
                    println!("close");
                }
                Err(err) => {
                    println!("ws message is err:{:?}", err);
                    break;
                }
                _ => {
                    println!("other ws message");
                }
            }
        }
    };
    pin_mut!(send_to_ws, receiv_from_ws);
    future::select(send_to_ws, receiv_from_ws).await;
}

use crate::proxy::InnerProxy::InnerProxy;
async fn work(
    trade_pair_arc: Arc<Mutex<TradePair>>,
    hwnd: usize,
    tx: UnboundedSender<Message>,
    rx: &mut UnboundedReceiver<Message>,
    proxy_str: &Option<String>,
) {
    let url = "wss://fstream.binance.com/ws".to_string();
    if !proxy_str.is_none() {
        let proxy_url = proxy_str.clone().unwrap();
        let proxy = match InnerProxy::from_proxy_str(&proxy_url) {
            Ok(proxy) => proxy,
            Err(_) => return,
        };
        let tcp_stream = match proxy.connect_async(&url).await {
            Ok(stream) => stream,
            Err(_) => return,
        };
        let (ws_stream, _) = match client_async_tls(&url, tcp_stream).await {
            Ok(stream) => stream,
            Err(_) => return,
        };
        ws_handle(
            ws_stream,
            Arc::clone(&trade_pair_arc),
            hwnd,
            tx.clone(),
            rx,
        )
        .await;
    } else {
        let (ws_stream, _) = match connect_async_tls_with_config(&url, None, true, None).await {
            Ok(stream) => stream,
            Err(_) => return,
        };
        ws_handle(
            ws_stream,
            Arc::clone(&trade_pair_arc),
            hwnd,
            tx.clone(),
            rx,
        )
        .await;
    }
}

async fn receive_from_ui(
    trade_pair_arc: Arc<Mutex<TradePair>>,
    hwnd: usize,
    mut receiver: tokio::sync::mpsc::Receiver<TradePair>,
    tx: UnboundedSender<Message>,
) {
    loop {
        while let Some(new_trade_pair) = receiver.recv().await {
            let mut last_trade_pair = trade_pair_arc.lock().unwrap();
            if *last_trade_pair == new_trade_pair {
                continue;
            }
            unsubscribe(&last_trade_pair, tx.clone());
            subscribe(&new_trade_pair, tx.clone());
            *last_trade_pair = new_trade_pair;
            send_message_to_ui(hwnd, ApiMessage::Notify("切换中...".to_string()));
        }
    }
}

fn subscribe(trade_pair: &TradePair, tx: UnboundedSender<Message>) {
    let ws_name = &TRADE_INFO.get(trade_pair).unwrap().ws_name.clone();
    let message_str = format!(
        r##"{{"method":"SUBSCRIBE","params":["{}"],"id": 1}}"##,
        ws_name
    );
    tx.unbounded_send(Message::Text(message_str)).unwrap();
}
fn unsubscribe(trade_pair: &TradePair, tx: UnboundedSender<Message>) {
    let ws_name = &TRADE_INFO.get(trade_pair).unwrap().ws_name.clone();
    let message_str = format!(
        r##"{{"method":"UNSUBSCRIBE","params":["{}"],"id": 1}}"##,
        ws_name
    );
    tx.unbounded_send(Message::Text(message_str)).unwrap();
}

pub async fn run(
    hwnd: HWND,
    receiver: tokio::sync::mpsc::Receiver<TradePair>,
    trade_pair: TradePair,
    proxy_str: Option<String>,
) {
    let (tx, mut rx) = futures_channel::mpsc::unbounded::<Message>();
    let trade_pair_arc = Arc::new(Mutex::new(trade_pair));
    tokio::spawn(receive_from_ui(
        Arc::clone(&trade_pair_arc),
        hwnd.0 as usize,
        receiver,
        tx.clone(),
    ));
    loop {
        work(
            Arc::clone(&trade_pair_arc),
            hwnd.0 as usize,
            tx.clone(),
            &mut rx,
            &proxy_str,
        )
        .await;
        send_message_to_ui(hwnd.0 as usize, ApiMessage::Notify("重连中...".to_string()));
        println!("Reconnect...");
    }
}
