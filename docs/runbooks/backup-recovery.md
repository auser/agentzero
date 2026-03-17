# Backup & Recovery Runbook

## Backup

### Manual Backup
```bash
# Export all encrypted data stores
agentzero backup export --output ./backup-$(date +%Y%m%d).tar.gz

# Export specific stores
agentzero backup export --store memory --output ./memory-backup.tar.gz
agentzero backup export --store skills --output ./skills-backup.tar.gz
```

### Scheduled Backup (via cron)
```bash
# Add a daily backup at 2am
agentzero cron add "0 2 * * *" "backup export --output /backups/agentzero-$(date +%Y%m%d).tar.gz"

# Verify backup schedule
agentzero cron list
```

### What's Backed Up
- SQLite conversation memory database
- Encrypted key store
- Skills state and installed skills
- Agent definitions
- IPC message queue
- Audit logs (if enabled)

## Recovery

### Restore from Backup
```bash
# Full restore
agentzero backup import --input ./backup-20260317.tar.gz

# Restore specific store
agentzero backup import --store memory --input ./memory-backup.tar.gz
```

### Verify Integrity
```bash
# Check database integrity after restore
agentzero doctor models
agentzero memory list --limit 1
agentzero status
```

### Recovery from Encryption Key Loss
If `~/.agentzero/.agentzero-data.key` is lost:
1. Encrypted data (memory, IPC) is unrecoverable
2. AgentZero will auto-recreate the database on next start
3. Previous conversation history will be lost
4. Re-install skills: `agentzero skill add <name>`

**Prevention**: Back up the key file separately:
```bash
cp ~/.agentzero/.agentzero-data.key /secure-backup/
```

## Encrypted Export Format
- Archive: gzip-compressed tar
- Each store is a separate file within the archive
- SQLite databases are exported as-is (encryption at rest via SQLCipher)
- Key file is NOT included in exports (must be backed up separately)
