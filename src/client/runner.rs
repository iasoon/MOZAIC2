use tokio::process;
use std::process::Stdio;
use tokio::io::{Lines, BufReader, AsyncBufReadExt, AsyncWriteExt};

pub struct Bot {
    pub name: String,
    pub argv: Vec<String>,
}

impl Bot {
    pub fn spawn_process(&self) -> BotProcess {
        let mut child = process::Command::new(&self.argv[0])
            .args(&self.argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawning failed");
        
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout).lines();

        return BotProcess {
            stdin: child.stdin.take().unwrap(),
            stdout: reader,
            child,
        };
    }
}

pub struct BotProcess {
    child: process::Child,
    stdin: process::ChildStdin,
    stdout: Lines<BufReader<process::ChildStdout>>,
}


impl BotProcess {
    pub async fn communicate(&mut self, input: &[u8]) -> String {
        self.stdin.write_all(input).await.expect("write failed");
        self.stdin.write_u8(b'\n').await.expect("write failed");
        let line = self.stdout.next_line().await.expect("read failed");
        return line.expect("no line found");
    }
}