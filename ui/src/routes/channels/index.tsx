import { createFileRoute } from '@tanstack/react-router'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  MessageCircle, Hash, Slack, Mail, Phone,
  Globe, Wifi, Terminal, Mic, Share2,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'

export const Route = createFileRoute('/channels/')({
  component: ChannelsPage,
})

interface ChannelDef {
  name: string
  label: string
  description: string
  icon: LucideIcon
  category: string
}

const CHANNELS: ChannelDef[] = [
  { name: 'telegram',       label: 'Telegram',        description: 'Bot API integration',          icon: MessageCircle, category: 'Messaging' },
  { name: 'discord',        label: 'Discord',          description: 'Text channels & threads',       icon: Hash,          category: 'Messaging' },
  { name: 'slack',          label: 'Slack',            description: 'Channels & threads',            icon: Slack,         category: 'Messaging' },
  { name: 'whatsapp',       label: 'WhatsApp',         description: 'Business API',                  icon: MessageCircle, category: 'Messaging' },
  { name: 'matrix',         label: 'Matrix',           description: 'Federated encrypted chat',      icon: Hash,          category: 'Messaging' },
  { name: 'signal',         label: 'Signal',           description: 'Private messaging',             icon: MessageCircle, category: 'Messaging' },
  { name: 'imessage',       label: 'iMessage',         description: 'Apple messaging via bridge',    icon: MessageCircle, category: 'Messaging' },
  { name: 'mattermost',     label: 'Mattermost',       description: 'Self-hosted Slack alternative', icon: Hash,          category: 'Messaging' },
  { name: 'irc',            label: 'IRC',              description: 'Legacy IRC protocol',           icon: Terminal,      category: 'Messaging' },
  { name: 'email',          label: 'Email',            description: 'SMTP/IMAP support',             icon: Mail,          category: 'Communication' },
  { name: 'sms',            label: 'SMS',              description: 'Carrier text messaging',        icon: Phone,         category: 'Communication' },
  { name: 'nostr',          label: 'Nostr',            description: 'Decentralised protocol',        icon: Share2,        category: 'Decentralised' },
  { name: 'webhook',        label: 'Webhook',          description: 'Generic inbound webhooks',      icon: Wifi,          category: 'Integration' },
  { name: 'cli',            label: 'CLI',              description: 'Terminal interaction',          icon: Terminal,      category: 'Integration' },
  { name: 'transcription',  label: 'Transcription',    description: 'Audio-to-text (local)',         icon: Mic,           category: 'Integration' },
  { name: 'lark',           label: 'Lark / Feishu',    description: 'ByteDance collaboration',       icon: Globe,         category: 'Enterprise' },
  { name: 'dingtalk',       label: 'DingTalk',         description: 'Alibaba enterprise',            icon: Globe,         category: 'Enterprise' },
  { name: 'nextcloud',      label: 'Nextcloud Talk',   description: 'Nextcloud video & chat',        icon: Globe,         category: 'Enterprise' },
]

function ChannelsPage() {
  const categories = [...new Set(CHANNELS.map((c) => c.category))]

  return (
    <div className="space-y-6 max-w-5xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Channels</h1>
        <p className="text-xs text-muted-foreground">Configure channel tokens in Config → channels_config</p>
      </div>

      {categories.map((cat) => (
        <div key={cat} className="space-y-3">
          <h2 className="text-sm font-medium text-muted-foreground">{cat}</h2>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {CHANNELS.filter((c) => c.category === cat).map((ch) => {
              const Icon = ch.icon
              return (
                <Card key={ch.name} className="bg-card">
                  <CardHeader className="pb-2 pt-4">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <Icon className="h-4 w-4 text-muted-foreground" />
                        <CardTitle className="text-sm">{ch.label}</CardTitle>
                      </div>
                      <Badge variant="outline" className="text-xs">
                        {ch.name}
                      </Badge>
                    </div>
                  </CardHeader>
                  <CardContent className="pb-4">
                    <p className="text-xs text-muted-foreground mb-3">{ch.description}</p>
                    <div className="text-xs text-muted-foreground font-mono bg-muted rounded px-2 py-1">
                      POST /v1/webhook/{ch.name}
                    </div>
                  </CardContent>
                </Card>
              )
            })}
          </div>
        </div>
      ))}

      <div className="rounded-lg border border-border p-4 text-sm space-y-2">
        <p className="font-medium">Configure channels</p>
        <p className="text-muted-foreground text-xs">
          Channel tokens and settings are configured in your <code className="bg-muted px-1 rounded">agentzero.toml</code>.
          Use the <a href="/config" className="text-primary hover:underline">Config</a> page to edit them,
          or run <code className="bg-muted px-1 rounded">agentzero channel add</code> from the CLI.
        </p>
      </div>
    </div>
  )
}
