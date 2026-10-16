//! Asks a model as the tutor would: `cargo run --release -p un-llm --features metal --example ask --
//! <model.gguf> <system-prompt.txt> "what the child says" ["what the tutor said before"]`. Several
//! lines of the child's, separated by `|`, are asked in turn as one conversation (the second and
//! later answers show what the kept context saves).

use std::path::Path;
use std::time::Instant;

use un_core::strategy::{ChatPrompt, ChatTurn};
use un_engine::{CancelToken, ChatModel};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: ask <model.gguf> <system.txt> <child[|child...]> [tutor]");
        std::process::exit(2);
    }
    let system = std::fs::read_to_string(&args[2]).expect("system prompt");
    let mut turns = Vec::new();
    if let Some(tutor) = args.get(4) {
        turns.push(ChatTurn { child: false, text: tutor.clone() });
    }
    let chat = un_llm::LlamaChat::shared(Path::new(&args[1])).expect("model");
    for line in args[3].split('|') {
        turns.push(ChatTurn { child: true, text: line.trim().to_string() });
        let prompt = ChatPrompt { system: system.clone(), turns: turns.clone(), max_tokens: 60 };
        let started = Instant::now();
        let answer = chat.reply(&prompt, &CancelToken::default()).expect("answer");
        println!("> {}\n{answer}  ({:.2} s)", line.trim(), started.elapsed().as_secs_f64());
        turns.push(ChatTurn { child: false, text: answer });
    }
}
