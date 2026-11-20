// SPDX-License-Identifier: GPL-3.0-or-later

//! Asks an external agent one thing over ACP and prints what it says and does: the recorded
//! session of P4-14, run by hand against a real agent.
//!
//! ```text
//! cargo run -p qs-acp --example ask -- --cwd <project> --prompt "<text>" -- <agent> [<arg>...]
//! ```
//!
//! For Claude Code through its ACP adapter the agent is
//! `npx @agentclientprotocol/claude-agent-acp`. A permission request is answered with the
//! agent's reject option, so the agent changes nothing; the session ends with the turn.

#![expect(
    clippy::print_stdout,
    reason = "a command-line example: its output is the recorded session"
)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use qs_acp::{Agent, AgentCommand, ClientInfo, Event, Files, OptionKind, Outcome, SessionId};

/// How long the agent may say nothing before the session is given up.
const QUIET: Duration = Duration::from_secs(300);

/// `ask --cwd <dir> --prompt <text> -- <agent> [<arg>...]`.
#[derive(clap::Parser, Debug)]
#[command(name = "ask", about = "Ask an external agent one thing over ACP")]
struct Args {
    /// The session's working directory.
    #[arg(long, default_value = ".")]
    cwd: PathBuf,
    /// What to ask.
    #[arg(long)]
    prompt: String,
    /// The agent's program and its arguments.
    #[arg(last = true, required = true)]
    agent: Vec<String>,
}

/// What the session does next after an event.
enum Next {
    /// Go on.
    Continue,
    /// The session is over, well or not.
    End(ExitCode),
}

/// Prints `event` and sends what the protocol asks for in answer to it.
fn take(agent: &mut Agent, args: &Args, session: &mut Option<SessionId>, event: Event) -> Next {
    let sent = match event {
        Event::Initialized { agent: name } => {
            println!("[agent] {}", name.as_deref().unwrap_or("unnamed"));
            agent.client().new_session(&args.cwd, &[])
        }
        Event::SessionReady(id) => {
            println!("[session] {}", id.as_str());
            let sent = agent.client().prompt(&id, &args.prompt);
            *session = Some(id);
            sent
        }
        Event::Message(text) => {
            print!("{text}");
            Ok(())
        }
        Event::Thought(text) => {
            println!("[thought] {text}");
            Ok(())
        }
        Event::ToolCall(call) | Event::ToolCallUpdate(call) => {
            println!(
                "\n[tool {}] {} {:?} {:?}",
                call.id.as_str(),
                call.title.as_deref().unwrap_or(""),
                call.status,
                call.paths
            );
            Ok(())
        }
        Event::Plan(entries) => {
            for entry in entries {
                println!("[plan] {} ({})", entry.content, entry.status);
            }
            Ok(())
        }
        Event::PermissionAsked {
            request,
            tool,
            options,
        } => {
            println!(
                "\n[permission] {}: rejected",
                tool.title.as_deref().unwrap_or("")
            );
            let outcome = options
                .into_iter()
                .find(|option| option.kind == OptionKind::RejectOnce)
                .map_or(Outcome::Cancelled, |option| Outcome::Selected(option.id));
            agent.client().answer(&request, &outcome)
        }
        // Never asked for: the example leaves the agent its own file tools.
        Event::ReadFile { request, .. } | Event::WriteFile { request, .. } => agent
            .client()
            .refuse_file(&request, "this client offers no file system"),
        Event::TurnEnded(reason) => {
            println!("\n[ended] {reason:?}");
            return Next::End(ExitCode::SUCCESS);
        }
        Event::Refused { method, message } => {
            println!("[refused] {method}: {message}");
            return Next::End(ExitCode::FAILURE);
        }
        Event::WrongVersion(version) => {
            println!("[version] the agent speaks protocol version {version}");
            return Next::End(ExitCode::FAILURE);
        }
        Event::Exited => {
            println!("[exited]");
            return Next::End(ExitCode::FAILURE);
        }
    };
    match sent {
        Ok(()) => Next::Continue,
        Err(error) => {
            println!("[error] {error}");
            Next::End(ExitCode::FAILURE)
        }
    }
}

fn main() -> ExitCode {
    let args = <Args as clap::Parser>::parse();
    let Some((program, rest)) = args.agent.split_first() else {
        return ExitCode::FAILURE;
    };
    let command = AgentCommand {
        program: PathBuf::from(program),
        args: rest.to_vec(),
        env: Vec::new(),
    };
    let (woken, wakes) = mpsc::channel();
    let started = Agent::spawn(&command, &args.cwd, move || {
        // The receiver is gone only once the session is over.
        let _gone = woken.send(());
    });
    let mut agent = match started {
        Ok(agent) => agent,
        Err(error) => {
            println!("[error] {error}");
            return ExitCode::FAILURE;
        }
    };
    let info = ClientInfo {
        name: "qs-acp-ask".to_owned(),
        title: "QuakeC Studio".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    if let Err(error) = agent.client().initialize(&info, Files::AgentsOwn) {
        println!("[error] {error}");
        return ExitCode::FAILURE;
    }
    let mut session = None;
    loop {
        if wakes.recv_timeout(QUIET).is_err() {
            println!("[quiet] the agent said nothing for {} s", QUIET.as_secs());
            return ExitCode::FAILURE;
        }
        for event in agent.poll() {
            if let Next::End(code) = take(&mut agent, &args, &mut session, event) {
                return code;
            }
        }
    }
}
