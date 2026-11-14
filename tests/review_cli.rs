use chrono::{DateTime, Utc};
use ship_signal::{AnalysisInput, Classification, Feedback, FeedbackSource, Need, Workflow};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
};
struct Source;
impl FeedbackSource for Source {
    fn read(&mut self, _: DateTime<Utc>, _: DateTime<Utc>) -> anyhow::Result<AnalysisInput> {
        Ok(AnalysisInput {
            feedback: vec![Feedback {
                product_id: "shop".into(),
                feedback_source_id: "email".into(),
                source_message_id: "1".into(),
                customer_id: Some("alice".into()),
                submitted_at: "2026-11-07T00:00:00Z".parse()?,
                text: "Please add CSV downloads".into(),
                source_reference: None,
            }],
            diagnostics: BTreeMap::new(),
        })
    }
}
#[test]
fn cli_shows_model_suggestion_and_saves_only_a_confirmed_match() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite");
    let mut workflow = Workflow::open(&path)?;
    workflow.assign_need_identity("shop", "Export data", "export")?;
    let report = workflow.analyze(
        &mut Source,
        "2026-11-06T00:00:00Z".parse()?,
        "2026-11-13T00:00:00Z".parse()?,
        |_| {
            Ok(vec![Classification {
                message_index: 0,
                needs: vec![Need {
                    name: "CSV download".into(),
                    variant: Some("CSV".into()),
                }],
            }])
        },
    )?;
    drop(workflow);
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<()> {
        listener.set_nonblocking(true)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    thread::sleep(std::time::Duration::from_millis(10))
                }
                Err(error) => return Err(error.into()),
            }
        };
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        let header_end = loop {
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                anyhow::bail!("incomplete HTTP request");
            }
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .map(|(_, v)| v.trim().parse())
            })
            .transpose()?
            .unwrap_or(0);
        while bytes.len() < header_end + length {
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                anyhow::bail!("incomplete HTTP body");
            }
            bytes.extend_from_slice(&buffer[..n]);
        }
        let answer = serde_json::json!({"kind":"existing","identity":"export","reason":"Both request exports"}).to_string();
        let body = serde_json::json!({"id":"resp_test","object":"response","created_at":0,"status":"completed","model":"test","output":[{"type":"message","id":"msg_test","role":"assistant","status":"completed","content":[{"type":"output_text","text":answer,"annotations":[]}]}],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}).to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(())
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_ship-signal"))
        .args([
            "review-needs",
            "--state",
            path.to_str().unwrap(),
            "--report",
            &report.id.to_string(),
            "--model",
            "test",
        ])
        .env("OPENAI_API_KEY", "test")
        .env("OPENAI_BASE_URL", format!("http://{address}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(b"1\n")?;
    let output = child.wait_with_output()?;
    server.join().unwrap()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("Please add CSV downloads"));
    assert!(stdout.contains("Suggested match: export"));
    assert!(stdout.contains("Saved reviewed report"));
    let workflow = Workflow::open(&path)?;
    assert!(workflow.need_review_queue(report.id)?.is_empty());
    assert_eq!(workflow.report(report.id)?, report);
    Ok(())
}
