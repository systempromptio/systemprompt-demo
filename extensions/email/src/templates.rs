//! The welcome / $5-credit email, rendered as an HTML + plain-text
//! multipart message.

use lettre::Message;
use lettre::message::Mailbox;
use lettre::message::header::ContentType;

use crate::error::EmailError;
use crate::palette::{
    ACCENT_TINT, BG_PAGE, BG_SURFACE, BORDER, BRAND_ORANGE, HEADER_BG, TEXT_BODY, TEXT_MUTED,
    TEXT_PRIMARY,
};

const FONT: &str =
    "-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif";

// Why: bump the bridge-v* tag in both URLs on every bridge release. The bridge
// ships under its own `bridge-v*` tag, so `releases/latest` resolves to the
// gateway release and 404s here.
pub const BRIDGE_MAC_URL: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-aarch64-apple-darwin-app.zip";
pub const BRIDGE_WINDOWS_URL: &str = "https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-x86_64-pc-windows-msvc.exe";

const SUBJECT: &str = "Your $5 systemprompt credit is ready";

pub fn build_welcome_email(
    from: Mailbox,
    to: Mailbox,
    display_name: &str,
    site_url: &str,
) -> Result<Message, EmailError> {
    let html_body = build_html_body(display_name, site_url);
    let plain_body = build_plain_body(display_name, site_url);

    Message::builder()
        .from(from)
        .to(to)
        .subject(SUBJECT)
        .multipart(
            lettre::message::MultiPart::alternative()
                .singlepart(
                    lettre::message::SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(plain_body),
                )
                .singlepart(
                    lettre::message::SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html_body),
                ),
        )
        .map_err(EmailError::from)
}

fn greeting(display_name: &str) -> String {
    if display_name.trim().is_empty() {
        "Welcome.".to_owned()
    } else {
        format!("Welcome, {}.", display_name.trim())
    }
}

fn build_plain_body(display_name: &str, site_url: &str) -> String {
    let setup_url = format!("{site_url}/setup");
    format!(
        "{greeting}\n\n\
         You've been given $5 of credit to try systemprompt with Claude Desktop.\n\
         No card, no setup fees — just connect and start building. Here's how:\n\n\
         1. DOWNLOAD THE BRIDGE\n\
         The Systemprompt Bridge connects Claude Desktop (or Cowork) to your account.\n\
         Mac:     {mac}\n\
         Windows: {win}\n\n\
         2. SIGN IN WITH YOUR CODE\n\
         Open {setup_url} and copy your one-time bridge sign-in code,\n\
         then paste it into the bridge on first launch.\n\n\
         3. OPEN CLAUDE DESKTOP\n\
         Once the bridge is signed in, Claude Desktop and Cowork are configured\n\
         automatically. Your $5 credit is applied to every request through the\n\
         governed gateway — watch it work, and stop worrying about surprise bills.\n\n\
         ---\n\n\
         systemprompt.io | the governed gateway for AI agents\n\
         Setup guide: {setup_url}",
        greeting = greeting(display_name),
        mac = BRIDGE_MAC_URL,
        win = BRIDGE_WINDOWS_URL,
        setup_url = setup_url,
    )
}

fn build_html_body(display_name: &str, site_url: &str) -> String {
    let setup_url = format!("{site_url}/setup");
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta name="color-scheme" content="light">
<title>Your $5 systemprompt credit is ready</title>
</head>
<body style="margin:0;padding:0;background-color:{BG_PAGE};-webkit-text-size-adjust:100%;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:{BG_PAGE};">
<tr>
<td align="center" style="padding:40px 16px;">
<table role="presentation" cellspacing="0" cellpadding="0" border="0" style="max-width:560px;width:100%;margin:0 auto;background-color:{BG_SURFACE};border:1px solid {BORDER};border-radius:8px;overflow:hidden;">
<tr>
<td style="background-color:{HEADER_BG};padding:24px 48px;">
<a href="{site_url}" style="text-decoration:none;">
<img src="https://systemprompt.io/files/images/logo.png" alt="systemprompt.io" width="200" height="33" style="display:block;border:0;outline:none;text-decoration:none;" />
</a>
</td>
</tr>
<tr>
<td style="padding:36px 48px 0 48px;">
<h1 style="margin:0;font-family:{FONT};font-size:26px;font-weight:700;line-height:1.3;color:{TEXT_PRIMARY};">{greeting}</h1>
</td>
</tr>
<tr>
<td style="padding:20px 48px 0 48px;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:{ACCENT_TINT};border-radius:8px;">
<tr><td style="padding:18px 20px;font-family:{FONT};font-size:16px;line-height:1.6;color:{TEXT_PRIMARY};">
<span style="font-weight:700;color:{BRAND_ORANGE};">You've been given $5 of credit</span> to try systemprompt with Claude Desktop. No card, no setup fees — just connect and start building.
</td></tr>
</table>
</td>
</tr>
{steps}
<tr>
<td style="padding:32px 48px 0 48px;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0">
<tr><td style="border-top:1px solid {BORDER};font-size:1px;line-height:1px;">&nbsp;</td></tr>
</table>
</td>
</tr>
<tr>
<td style="padding:20px 48px 36px 48px;font-family:{FONT};font-size:14px;line-height:1.7;color:{TEXT_MUTED};">
Need a hand? Everything is on your <a href="{setup_url}" style="color:{BRAND_ORANGE};font-weight:500;text-decoration:none;">setup page</a>.
</td>
</tr>
</table>
<table role="presentation" cellspacing="0" cellpadding="0" border="0" style="max-width:560px;width:100%;margin:0 auto;">
<tr>
<td align="center" style="padding:24px 48px;font-family:{FONT};font-size:12px;line-height:1.6;color:{TEXT_MUTED};">
systemprompt.io | the governed gateway for AI agents
</td>
</tr>
</table>
</td>
</tr>
</table>
</body>
</html>"#,
        greeting = greeting(display_name),
        steps = html_steps(&setup_url),
    )
}

fn html_steps(setup_url: &str) -> String {
    let step1 = html_step(
        "1",
        "Download the Bridge",
        &format!(
            r#"The Systemprompt Bridge connects Claude Desktop (or Cowork) to your account.<br>
<a href="{BRIDGE_MAC_URL}" style="color:{BRAND_ORANGE};font-weight:600;text-decoration:none;">Download for Mac &#8594;</a> &nbsp; <a href="{BRIDGE_WINDOWS_URL}" style="color:{BRAND_ORANGE};font-weight:600;text-decoration:none;">Download for Windows &#8594;</a>"#,
        ),
    );
    let step2 = html_step(
        "2",
        "Sign in with your code",
        &format!(
            r#"Open your <a href="{setup_url}" style="color:{BRAND_ORANGE};font-weight:600;text-decoration:none;">setup page</a>, copy your one-time bridge sign-in code, and paste it into the bridge on first launch."#,
        ),
    );
    let step3 = html_step(
        "3",
        "Open Claude Desktop",
        "Once the bridge is signed in, Claude Desktop and Cowork are configured automatically. Your $5 credit is applied to every request through the governed gateway.",
    );
    format!("{step1}\n{step2}\n{step3}")
}

fn html_step(number: &str, title: &str, body: &str) -> String {
    format!(
        r#"<tr>
<td style="padding:24px 48px 0 48px;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0">
<tr>
<td width="28" valign="top" style="font-family:{FONT};font-size:15px;font-weight:700;color:{BRAND_ORANGE};line-height:1.7;">{number}.</td>
<td style="font-family:{FONT};font-size:15px;line-height:1.7;color:{TEXT_BODY};">
<span style="font-weight:600;color:{TEXT_PRIMARY};">{title}</span><br>
{body}
</td>
</tr>
</table>
</td>
</tr>"#
    )
}
