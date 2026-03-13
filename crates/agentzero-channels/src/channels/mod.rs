pub mod channel_setup;
pub mod helpers;

// ---------------------------------------------------------------------------
// Macro 1: channel_stub! — one-liner for unimplemented channels
// Generates struct, descriptor, and async Channel impl that bail!s.
// ---------------------------------------------------------------------------

#[allow(unused_macros)]
macro_rules! channel_stub {
    ($name:ident, $descriptor:ident, $id:literal, $display:literal) => {
        #[allow(dead_code)]
        pub struct $name;

        pub const $descriptor: crate::ChannelDescriptor = crate::ChannelDescriptor {
            id: $id,
            display_name: $display,
        };

        #[async_trait::async_trait]
        impl crate::Channel for $name {
            fn name(&self) -> &str {
                $id
            }

            async fn send(&self, _message: &crate::SendMessage) -> anyhow::Result<()> {
                anyhow::bail!("channel `{}` is not implemented yet", $id)
            }

            async fn listen(
                &self,
                _tx: tokio::sync::mpsc::Sender<crate::ChannelMessage>,
            ) -> anyhow::Result<()> {
                anyhow::bail!("channel `{}` is not implemented yet", $id)
            }

            async fn health_check(&self) -> bool {
                false
            }
        }
    };
}

#[allow(unused_imports)]
pub(crate) use channel_stub;

// ---------------------------------------------------------------------------
// Macro 2: channel_meta! — descriptor const for implemented channels.
// The struct and Channel impl are written manually in the same file.
// ---------------------------------------------------------------------------

macro_rules! channel_meta {
    ($descriptor:ident, $id:literal, $display:literal) => {
        pub const $descriptor: crate::ChannelDescriptor = crate::ChannelDescriptor {
            id: $id,
            display_name: $display,
        };
    };
}

pub(crate) use channel_meta;

// ---------------------------------------------------------------------------
// Macro 3: channel_catalog! — auto-wires module tree + catalog array.
// Adding a new channel = add one line here + create the file.
// ---------------------------------------------------------------------------

macro_rules! channel_catalog {
    ($( $module:ident => ($name:ident, $descriptor:ident) ),+ $(,)?) => {
        $(mod $module;)+

        $(#[allow(unused_imports)] pub use $module::$name;)+
        $(use $module::$descriptor;)+

        pub const CHANNEL_CATALOG: &[crate::ChannelDescriptor] = &[
            $($descriptor,)+
        ];
    };
}

// ---------------------------------------------------------------------------
// Channel catalog — the single place to register all channels.
// To add a new channel:
//   1. Create src/channels/my_channel.rs with channel_stub! or channel_meta!
//   2. Add one line below: my_channel => (MyChannel, MY_CHANNEL_DESCRIPTOR),
// ---------------------------------------------------------------------------

channel_catalog!(
    cli               => (CliChannel, CLI_DESCRIPTOR),
    telegram          => (TelegramChannel, TELEGRAM_DESCRIPTOR),
    discord           => (DiscordChannel, DISCORD_DESCRIPTOR),
    slack             => (SlackChannel, SLACK_DESCRIPTOR),
    mattermost        => (MattermostChannel, MATTERMOST_DESCRIPTOR),
    imessage          => (ImessageChannel, IMESSAGE_DESCRIPTOR),
    matrix            => (MatrixChannel, MATRIX_DESCRIPTOR),
    signal            => (SignalChannel, SIGNAL_DESCRIPTOR),
    whatsapp          => (WhatsappChannel, WHATSAPP_DESCRIPTOR),
    mqtt              => (MqttChannel, MQTT_DESCRIPTOR),
    transcription     => (TranscriptionChannel, TRANSCRIPTION_DESCRIPTOR),
    whatsapp_storage  => (WhatsappStorageChannel, WHATSAPP_STORAGE_DESCRIPTOR),
    whatsapp_web      => (WhatsappWebChannel, WHATSAPP_WEB_DESCRIPTOR),
    linq              => (LinqChannel, LINQ_DESCRIPTOR),
    wati              => (WatiChannel, WATI_DESCRIPTOR),
    nextcloud_talk    => (NextcloudTalkChannel, NEXTCLOUD_TALK_DESCRIPTOR),
    email             => (EmailChannel, EMAIL_DESCRIPTOR),
    irc               => (IrcChannel, IRC_DESCRIPTOR),
    lark              => (LarkChannel, LARK_DESCRIPTOR),
    feishu            => (FeishuChannel, FEISHU_DESCRIPTOR),
    dingtalk          => (DingtalkChannel, DINGTALK_DESCRIPTOR),
    qq_official       => (QqOfficialChannel, QQ_OFFICIAL_DESCRIPTOR),
    nostr             => (NostrChannel, NOSTR_DESCRIPTOR),
    clawdtalk         => (ClawdtalkChannel, CLAWDTALK_DESCRIPTOR),
    webhook           => (WebhookChannel, WEBHOOK_DESCRIPTOR),
    napcat            => (NapcatChannel, NAPCAT_DESCRIPTOR),
    acp               => (AcpChannel, ACP_DESCRIPTOR),
    sms               => (SmsChannel, SMS_DESCRIPTOR),
);
