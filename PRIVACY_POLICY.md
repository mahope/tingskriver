# Tingskriver Privacy Policy

*Last updated: [Current Date]*

## Our Privacy-First Commitment

Tingskriver is built on the principle that your meeting data should remain private and under your control. This privacy policy explains how we handle data in our open-source meeting assistant.

## Data Processing Philosophy

### Local-First Processing
- **Meeting transcription**: Processed entirely on your device using local Whisper models
- **Audio recordings**: Never transmitted to external servers
- **Meeting content**: Remains on your infrastructure
- **AI summaries**: Generated locally or through your chosen LLM provider

### Your Data Ownership
- You own all meeting data, transcripts, and recordings
- Data is stored locally on your device
- No vendor lock-in - export your data anytime
- Complete control over data retention and deletion

## Usage Analytics

### What We Collect
Usage analytics is optional and off by default. When you choose to enable it, Tingskriver collects minimal, anonymized usage data:

**Application Usage:**
- Feature usage patterns (which tools you use most)
- Session duration and frequency
- Performance metrics (transcription success rates, error frequencies)
- UI interaction patterns (button clicks, navigation flows)

**Technical Metrics:**
- Application version and platform information
- Error logs and crash reports (anonymized)
- Performance benchmarks (processing times, resource usage)

### What We DON'T Collect
We never collect:
- ❌ Meeting content, transcripts, or recordings
- ❌ Personal information or identifiable data
- ❌ File names, meeting titles, or metadata
- ❌ Audio data or voice patterns
- ❌ Participant names or contact information
- ❌ LLM conversations or AI-generated content

### Why We Collect This Data
When enabled, analytics helps us with:
- **Product Quality**: Identifying and fixing bugs that impact user experience
- **Performance Optimization**: Understanding resource usage and system bottlenecks
- **Security**: Detecting potential security issues and vulnerabilities
- **Feature Development**: Making data-driven decisions about new features
- **Open Source Sustainability**: Ensuring the project meets user needs effectively

### Analytics Implementation
- **Provider**: PostHog (privacy-focused analytics platform)
- **Default**: Off by default; analytics starts only after you enable it in settings
- **Anonymization**: All data linked to generated user IDs only - no personal identification
- **Data retention**: 12 months maximum, then automatically deleted
- **Encryption**: All data encrypted in transit using industry-standard protocols
- **Location**: Data processed in accordance with PostHog's privacy policy
- **Access Control**: Strictly limited to core development team members

## Logs and Crash Dumps

Tingskriver keeps a log file and, after a crash, a crash dump on your own computer. Neither is uploaded or sent anywhere; you decide whether to send one to the developer.

- **Log**: `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs`, at most three files of 5 MB. It records what the app did (startup, devices, errors, panics with a backtrace), not what was said in a meeting.
- **Crash dumps**: when Tingskriver crashes in native code, Windows Error Reporting writes a minidump (`DumpType` 1: threads, call stacks and loaded modules, no full memory) to `%LOCALAPPDATA%\dk.mahoje.tingskriver\dumps`, at most five. The app sets this up with the per-user registry key `HKCU\Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\tingskriver.exe`. A minidump holds the program's state at the moment of the crash and can contain fragments of what it was working on. On by default; turn it off under Settings > Preferences > "Gem nedbrudsfiler", which removes the registry key.
- **Database backups**: if the database cannot be opened at startup, a copy is placed in `backups` in the app's data folder before the error is shown. Nothing is deleted.

## Third-Party Services

### LLM Providers (Optional)
If you choose to use external LLM providers:
- **Anthropic Claude**: Subject to Anthropic's privacy policy
- **Groq**: Subject to Groq's privacy policy
- **Local Ollama**: Processed entirely on your device

### Analytics Service (Optional)
- **PostHog**: Used for usage analytics when enabled
- **Data**: Only anonymized usage patterns, no meeting content
- **Control**: Completely optional, off by default, and user-controlled

## Your Privacy Rights

### Data Control
- **Access**: View all data stored locally on your device
- **Export**: Export your data in standard formats
- **Delete**: Remove all data from your device


### Analytics Transparency
- **Open source**: Full analytics implementation available for review in our source code
- **Opt-in**: New and existing installs have analytics disabled until you turn it on
- **Questions**: Contact us for any analytics-related concerns

## Data Security

### Local Security
- Data encrypted at rest using your device's security features
- No transmission of sensitive meeting data
- Standard file system permissions protect your data

### Open Source Transparency
- Full source code available for security review
- Community-audited privacy implementations
- No hidden data collection or tracking

## Changes to This Policy

We will notify users of any material changes to this privacy policy through:
- Updates to this document in our GitHub repository
- Release notes for application updates
- In-app notifications for significant privacy changes

## Contact Us

For privacy-related questions or concerns:
- **GitHub Issues**: [Create an issue](https://github.com/mahope/tingskriver/issues)
- **Community**: [Discord](https://discord.gg/crRymMQBFH)

## Open Source Commitment

As an open-source project under MIT license, you can:
- Review our complete privacy implementation
- Modify data handling to meet your requirements
- Deploy entirely on your own infrastructure
- Contribute to privacy improvements

---

*This privacy policy applies to Tingskriver v0.0.5 and later versions. For enterprise deployments, additional privacy controls may be available.*
