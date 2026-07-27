//! The internal "someone registered" notice.
//!
//! Goes to the reviewer, not the applicant. Registration is open but every
//! account is held until a human approves it, so this email carries everything
//! the decision needs — who they say they are and what they want to evaluate —
//! and the statement that approves them.

use lettre::Message;
use lettre::message::Mailbox;
use lettre::message::header::ContentType;

use crate::error::EmailError;
use crate::palette::{BG_PAGE, BG_SURFACE, BORDER, TEXT_MUTED, TEXT_PRIMARY};

const FONT: &str =
    "-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif";

#[derive(Debug, Clone)]
pub struct RegistrationNotice<'a> {
    pub name: &'a str,
    pub email: &'a str,
    pub company: &'a str,
    pub role: &'a str,
    pub team_size: &'a str,
    pub why_assessing: &'a str,
    pub credit_plans: Option<&'a str>,
}

pub fn build_registration_notice_email(
    from: Mailbox,
    to: Mailbox,
    notice: &RegistrationNotice<'_>,
    site_url: &str,
) -> Result<Message, EmailError> {
    let subject = format!("Access request: {} ({})", notice.company, notice.email);
    let review_url = format!(
        "{site_url} — approve with: UPDATE user_approvals SET status='approved', \
         decided_at=NOW() WHERE user_id=(SELECT id FROM users WHERE email='{email}');",
        email = notice.email,
    );

    Message::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .multipart(
            lettre::message::MultiPart::alternative()
                .singlepart(
                    lettre::message::SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(build_plain_body(notice, &review_url)),
                )
                .singlepart(
                    lettre::message::SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(build_html_body(notice, &review_url)),
                ),
        )
        .map_err(EmailError::from)
}

fn build_plain_body(notice: &RegistrationNotice<'_>, review_url: &str) -> String {
    format!(
        "New access request awaiting review.\n\n\
         Name:       {name}\n\
         Email:      {email}\n\
         Company:    {company}\n\
         Role:       {role}\n\
         Team size:  {team_size}\n\n\
         What they are evaluating:\n{why}\n\n\
         How they plan to use the credit:\n{plans}\n\n\
         Approve or ignore:\n{review_url}\n\n\
         No credit is granted and no access is given until you approve.",
        name = notice.name,
        email = notice.email,
        company = notice.company,
        role = notice.role,
        team_size = notice.team_size,
        why = notice.why_assessing,
        plans = notice.credit_plans.unwrap_or("(not given)"),
        review_url = review_url,
    )
}

fn build_html_body(notice: &RegistrationNotice<'_>, review_url: &str) -> String {
    let rows = [
        ("Name", notice.name),
        ("Email", notice.email),
        ("Company", notice.company),
        ("Role", notice.role),
        ("Team size", notice.team_size),
        ("Evaluating", notice.why_assessing),
        ("Credit plans", notice.credit_plans.unwrap_or("(not given)")),
    ]
    .iter()
    .map(|(label, value)| detail_row(label, value))
    .collect::<Vec<_>>()
    .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta name="color-scheme" content="light">
<title>Access request</title>
</head>
<body style="margin:0;padding:0;background-color:{BG_PAGE};-webkit-text-size-adjust:100%;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:{BG_PAGE};">
<tr>
<td align="center" style="padding:40px 16px;">
<table role="presentation" cellspacing="0" cellpadding="0" border="0" style="max-width:560px;width:100%;margin:0 auto;background-color:{BG_SURFACE};border:1px solid {BORDER};border-radius:8px;overflow:hidden;">
<tr>
<td style="padding:32px 40px 8px 40px;font-family:{FONT};">
<h1 style="margin:0;font-size:20px;font-weight:700;line-height:1.3;color:{TEXT_PRIMARY};">Access request awaiting review</h1>
</td>
</tr>
<tr>
<td style="padding:16px 40px 0 40px;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="font-family:{FONT};font-size:14px;line-height:1.6;">
{rows}
</table>
</td>
</tr>
<tr>
<td style="padding:28px 40px 36px 40px;font-family:{FONT};font-size:14px;line-height:1.7;">
<p style="margin:0;font-family:monospace;font-size:12px;line-height:1.6;color:{TEXT_PRIMARY};word-break:break-all;">{review_url}</p>
<p style="margin:12px 0 0 0;color:{TEXT_MUTED};font-size:13px;">No credit is granted and no access is given until you approve.</p>
</td>
</tr>
</table>
</td>
</tr>
</table>
</body>
</html>"#
    )
}

fn detail_row(label: &str, value: &str) -> String {
    let value = escape_html(value);
    format!(
        r#"<tr>
<td width="110" valign="top" style="padding:6px 12px 6px 0;color:{TEXT_MUTED};">{label}</td>
<td valign="top" style="padding:6px 0;color:{TEXT_PRIMARY};">{value}</td>
</tr>"#
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
