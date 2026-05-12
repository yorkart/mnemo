mod cli;
mod client;
mod commands;
mod helpers;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    mnemo_telemetry::init();
    let cli = cli::Cli::parse();
    commands::run(cli).await
}

#[cfg(test)]
mod tests {
    use super::helpers::*;
    use mnemo_domain::Namespace;

    #[test]
    fn parses_full_namespace_path() {
        let namespace = parse_namespace("default/u1/w1/t1/a1/cli");
        assert_eq!(namespace.tenant_id.as_deref(), Some("default"));
        assert_eq!(namespace.user_id.as_deref(), Some("u1"));
        assert_eq!(namespace.workspace_id.as_deref(), Some("w1"));
        assert_eq!(namespace.thread_id.as_deref(), Some("t1"));
        assert_eq!(namespace.agent_id, None);
        assert_eq!(namespace.source, None);
    }

    #[test]
    fn namespace_query_url_encodes_values() {
        let namespace = Namespace {
            tenant_id: Some("default".to_string()),
            user_id: Some("u1".to_string()),
            workspace_id: Some("w 1".to_string()),
            thread_id: None,
            agent_id: None,
            source: Some("cli test".to_string()),
        };
        let query = namespace_query(&namespace, &[("cursor", "page:20&x=y".to_string())]);
        assert_eq!(
            query,
            "tenant_id=default&user_id=u1&workspace_id=w+1&thread_id=&agent_id=&source=cli+test&cursor=page%3A20%26x%3Dy"
        );
    }
}
