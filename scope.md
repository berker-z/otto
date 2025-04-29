# OTTO - Sovereign Native Chat Client

## Core Purpose
- Sovereign, native chat client
- Modular, backend-agnostic
- Assist in technical, philosophical, literary conversations without censorship or compromise
- Personal, persistent assistant model ("otto") defined by user prompts and context trees

---

## Backend Architecture
- `LanguageModel` trait
  - Defines: `complete(&self, prompt: &str) -> Result<String, LlmError>`
- Each backend implements `LanguageModel`:
  - `OpenAIClient`
  - `ClaudeClient`
  - `DeepSeekClient`
  - `LocalLlamaClient`
  - `FakeClient` (for testing)
- Runtime selection of backend
- Modular backend design
- `LlmError` enum for clean error handling

---

## Session and Memory System
- **System Prompt**: Base persona (identity/personality) loaded at session start
- **Stacked Context**: Additional prompts can be layered:
  - Development assistant mode
  - Project-specific modes (e.g., godot project assistant)
  - Literature/media analysis mode
- **Tree-like Inheritance**:
  - Base → Persona → Project sub-persona → Current conversation
- **Memory**:
  - Metadata about each session
  - Active system prompt
  - Chosen backend
  - Optional project data
- Memories editable per chat, swappable, exportable

---

## Frontend Architecture
- Built with `iced`
- Two-panel layout:
  - Left: Chat list and settings access
  - Right: Chat window (prompt input, message history)
- Dark theme enforced
- Custom font: Iosevka Nerd Font Mono

---

## Frontend Dynamic Behavior
- Start new chat (system prompt + backend)
- Continue old chats with restored memory state
- Select chats from left panel
- Real-time message sending and receiving
- Smooth loading / backend call placeholders

---

## Storage Model
- File-based storage (no database for now)
- JSON files per chat
- Config file for saved API keys and settings

---

## Settings System
- API key entry for each backend
- Active backend selector
- System prompt editor
- Save/load personas and memory trees
- Optional UI toggles (font size, etc.)

---

## Deployment Model
- Compiled native binaries for:
  - Linux (primary)
  - Windows (optional, cross-compilation)
- Fully self-contained
- Optional local LLM integration

---

# Initial Roadmap

### Phase 1: Core Backend
- Define `LanguageModel` trait
- Create `FakeClient`
- Create `LlmError`
- Manually test backend response (stdout)

### Phase 2: Core Frontend
- Initialize iced window
- Two-panel layout
- Hardcoded prompt sending via button

### Phase 3: Integrate Backend with Frontend
- Prompt entry
- Backend call
- Display response dynamically

### Phase 4: Memory System
- System prompts and persona layering
- Save/load memory per session

### Phase 5: Settings
- API key entry
- Backend selector

### Phase 6: Storage
- Save chats and session metadata locally

### Phase 7: Frontend Refinements
- Scrollable chats
- Message grouping
- Lightweight UI transitions

---

# Guiding Principles
- Backend-agnostic, model-agnostic
- Persona and memory driven sessions
- Clean modular architecture
- Human-readable, maintainable Rust code
- Minimal dependency footprint
- Low memory usage
- Elegant but minimal interface
- Full user data ownership
