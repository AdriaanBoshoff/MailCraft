//! Give a `.msg` an RTF body: reads a `.msg` our writer made, replaces its body with the RTF in a
//! file, and writes the result. It is a tool for making synthetic messages to compare how an RTF
//! body is shown; the bodies are our own text.
//!
//! `cargo run -p openmailclient-msg --example msg_with_rtf -- in.msg body.rtf out.msg`

use openmailclient_model::{PropValue, pid, rtf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [source, body, target] = args.as_slice() else {
        eprintln!("usage: msg_with_rtf <in.msg> <body.rtf> <out.msg>");
        return;
    };
    let (data, body) = match (std::fs::read(source), std::fs::read(body)) {
        (Ok(data), Ok(body)) => (data, body),
        (Err(e), _) | (_, Err(e)) => return eprintln!("{e}"),
    };
    let mut item = match openmailclient_msg::read(&data) {
        Ok(item) => item,
        Err(e) => return eprintln!("{source}: {e}"),
    };
    item.props.remove(pid::BODY);
    item.props.remove(pid::HTML);
    item.props.set(pid::RTF_COMPRESSED, PropValue::Binary(rtf::store(&body)));
    // Marked as in sync, so that the RTF is the body that is shown ([MS-OXCMSG] 2.2.1.55.5).
    item.props.set(pid::RTF_IN_SYNC, PropValue::Bool(true));
    match openmailclient_msg::write(&item).map(|out| std::fs::write(target, out)) {
        Ok(Ok(())) => println!("wrote {target}"),
        Ok(Err(e)) => eprintln!("{target}: {e}"),
        Err(e) => eprintln!("{target}: {e}"),
    }
}
