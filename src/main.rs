#![windows_subsystem = "windows"]
mod my_window;
mod proxy;
use my_window::Window;
use anyhow::Result;
mod api;
use tokio::runtime::Runtime;
use windows::Win32::Foundation::HWND;
use std::{ffi::c_void, thread};
use tokio::sync::mpsc;
use clap::Parser;


/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    proxy: Option<String>,
}
fn main() -> Result<()> {
    

    let args = Args::parse();
    let (tx, rx):(mpsc::Sender<api::TradePair>, mpsc::Receiver<api::TradePair>) = mpsc::channel(1);
    
    let mut window = Window::new(None, None, None, tx, api::TradePair::BTCUSDT);
    window.init_window()?;
    let hwnd_v = window.hwnd;
    thread::spawn(move || {
        let rt = Runtime::new().expect("Runtime::new fail");
        rt.block_on( api::run(HWND(hwnd_v as *mut c_void), 
            rx, api::TradePair::BTCUSDT, args.proxy));
    });
    window.run_window()
}
