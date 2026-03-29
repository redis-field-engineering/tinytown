/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Agent definitions and lifecycle management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(Uuid);

impl AgentId {
    /// Create a new random agent ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID.
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the raw bytes of the underlying UUID.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }

    /// Return the first 4 hex characters of the UUID for compact display.
    #[must_use]
    pub fn short_id(&self) -> String {
        self.0.to_string().chars().take(4).collect()
    }

    /// Create a well-known ID for the supervisor.
    #[must_use]
    pub fn supervisor() -> Self {
        // Fixed UUID for supervisor
        Self(Uuid::from_bytes([
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01,
        ]))
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for AgentId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Agent types in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    /// Supervisor agent - coordinates workers
    Supervisor,
    /// Worker agent - executes tasks
    #[default]
    Worker,
}

/// How an agent session was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpawnMode {
    /// Fresh agent with no inherited context.
    #[default]
    Fresh,
    /// Agent forked from a parent with inherited context.
    ForkedContext,
    /// Agent resumed from a previous session.
    Resumed,
}

impl std::fmt::Display for SpawnMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fresh => write!(f, "fresh"),
            Self::ForkedContext => write!(f, "forkedcontext"),
            Self::Resumed => write!(f, "resumed"),
        }
    }
}

/// Top 200 boys' names from the 1920s (when Tiny Town, Colorado was founded).
/// Source: U.S. Social Security Administration, popular names by decade.
const NICKNAMES_1920S_BOYS: &[&str] = &[
    "Robert",
    "John",
    "James",
    "William",
    "Charles",
    "George",
    "Joseph",
    "Richard",
    "Edward",
    "Donald",
    "Thomas",
    "Frank",
    "Harold",
    "Paul",
    "Raymond",
    "Walter",
    "Jack",
    "Henry",
    "Kenneth",
    "Arthur",
    "Albert",
    "David",
    "Harry",
    "Eugene",
    "Ralph",
    "Howard",
    "Carl",
    "Willie",
    "Louis",
    "Clarence",
    "Earl",
    "Roy",
    "Fred",
    "Joe",
    "Francis",
    "Lawrence",
    "Herbert",
    "Leonard",
    "Ernest",
    "Alfred",
    "Anthony",
    "Stanley",
    "Norman",
    "Gerald",
    "Daniel",
    "Samuel",
    "Bernard",
    "Billy",
    "Melvin",
    "Marvin",
    "Warren",
    "Michael",
    "Leroy",
    "Russell",
    "Leo",
    "Andrew",
    "Edwin",
    "Elmer",
    "Peter",
    "Floyd",
    "Lloyd",
    "Ray",
    "Frederick",
    "Theodore",
    "Clifford",
    "Vernon",
    "Herman",
    "Clyde",
    "Chester",
    "Philip",
    "Alvin",
    "Lester",
    "Wayne",
    "Vincent",
    "Gordon",
    "Leon",
    "Lewis",
    "Charlie",
    "Glenn",
    "Calvin",
    "Martin",
    "Milton",
    "Lee",
    "Jesse",
    "Dale",
    "Cecil",
    "Bill",
    "Harvey",
    "Roger",
    "Victor",
    "Benjamin",
    "Ronald",
    "Wallace",
    "Sam",
    "Allen",
    "Arnold",
    "Willard",
    "Gilbert",
    "Edgar",
    "Oscar",
    "Gene",
    "Jerry",
    "Douglas",
    "Johnnie",
    "Claude",
    "Don",
    "Eddie",
    "Roland",
    "Everett",
    "Maurice",
    "Curtis",
    "Marion",
    "Virgil",
    "Wilbur",
    "Manuel",
    "Stephen",
    "Jerome",
    "Homer",
    "Leslie",
    "Glen",
    "Jessie",
    "Hubert",
    "Jose",
    "Jimmie",
    "Sidney",
    "Morris",
    "Hugh",
    "Max",
    "Bobby",
    "Bob",
    "Nicholas",
    "Luther",
    "Bruce",
    "Junior",
    "Wesley",
    "Alexander",
    "Rudolph",
    "Franklin",
    "Tom",
    "Irving",
    "Horace",
    "Willis",
    "Patrick",
    "Steve",
    "Johnny",
    "Dean",
    "Julius",
    "Keith",
    "Oliver",
    "Earnest",
    "Ben",
    "Jim",
    "Tony",
    "Edmund",
    "Lyle",
    "Guy",
    "Salvatore",
    "Orville",
    "Delbert",
    "Billie",
    "Phillip",
    "Clayton",
    "Otis",
    "Archie",
    "Alex",
    "Angelo",
    "Mike",
    "Jacob",
    "Clifton",
    "Bennie",
    "Matthew",
    "Duane",
    "Clinton",
    "Dennis",
    "Wilbert",
    "Dan",
    "Jay",
    "Marshall",
    "Leland",
    "Merle",
    "Ira",
    "Nathaniel",
    "Ivan",
    "Ervin",
    "Jimmy",
    "Irvin",
    "Alton",
    "Lowell",
    "Larry",
    "Dewey",
    "Emil",
    "Antonio",
    "Wilfred",
    "Elbert",
    "Juan",
    "Alan",
    "Allan",
    "Lonnie",
    "Nelson",
    "Forrest",
];

/// Top 200 girls' names from the 1920s (when Tiny Town, Colorado was founded).
/// Source: U.S. Social Security Administration, popular names by decade.
const NICKNAMES_1920S_GIRLS: &[&str] = &[
    "Mary",
    "Dorothy",
    "Helen",
    "Betty",
    "Margaret",
    "Ruth",
    "Virginia",
    "Doris",
    "Mildred",
    "Frances",
    "Elizabeth",
    "Evelyn",
    "Anna",
    "Alice",
    "Marie",
    "Jean",
    "Shirley",
    "Barbara",
    "Irene",
    "Marjorie",
    "Lois",
    "Florence",
    "Martha",
    "Rose",
    "Lillian",
    "Louise",
    "Catherine",
    "Ruby",
    "Patricia",
    "Eleanor",
    "Gladys",
    "Annie",
    "Josephine",
    "Thelma",
    "Edna",
    "Norma",
    "Pauline",
    "Lucille",
    "Gloria",
    "Edith",
    "Ethel",
    "Phyllis",
    "Grace",
    "Hazel",
    "June",
    "Bernice",
    "Marion",
    "Dolores",
    "Rita",
    "Lorraine",
    "Ann",
    "Esther",
    "Beatrice",
    "Juanita",
    "Geraldine",
    "Clara",
    "Jane",
    "Sarah",
    "Emma",
    "Joan",
    "Joyce",
    "Nancy",
    "Katherine",
    "Gertrude",
    "Elsie",
    "Julia",
    "Wilma",
    "Agnes",
    "Marian",
    "Bertha",
    "Eva",
    "Willie",
    "Audrey",
    "Theresa",
    "Vivian",
    "Wanda",
    "Laura",
    "Charlotte",
    "Ida",
    "Elaine",
    "Marilyn",
    "Anne",
    "Maxine",
    "Kathryn",
    "Kathleen",
    "Viola",
    "Pearl",
    "Vera",
    "Bessie",
    "Beverly",
    "Myrtle",
    "Alma",
    "Violet",
    "Nellie",
    "Ella",
    "Lillie",
    "Jessie",
    "Jeanne",
    "Eileen",
    "Ellen",
    "Lucy",
    "Minnie",
    "Sylvia",
    "Donna",
    "Rosemary",
    "Leona",
    "Stella",
    "Margie",
    "Mattie",
    "Genevieve",
    "Mabel",
    "Janet",
    "Bonnie",
    "Geneva",
    "Carol",
    "Georgia",
    "Carolyn",
    "Velma",
    "Lena",
    "Mae",
    "Maria",
    "Jennie",
    "Christine",
    "Peggy",
    "Arlene",
    "Marguerite",
    "Opal",
    "Sara",
    "Loretta",
    "Harriet",
    "Rosa",
    "Muriel",
    "Eunice",
    "Jeanette",
    "Blanche",
    "Carrie",
    "Emily",
    "Billie",
    "Beulah",
    "Dora",
    "Roberta",
    "Naomi",
    "Hilda",
    "Jacqueline",
    "Anita",
    "Alberta",
    "Inez",
    "Delores",
    "Fannie",
    "Hattie",
    "Lula",
    "Verna",
    "Cora",
    "Constance",
    "Madeline",
    "Miriam",
    "Ada",
    "Claire",
    "Mamie",
    "Lola",
    "Rosie",
    "Erma",
    "Rachel",
    "Mable",
    "Flora",
    "Daisy",
    "Sally",
    "Marcella",
    "Bette",
    "Olga",
    "Caroline",
    "Laverne",
    "Sophie",
    "Nora",
    "Rebecca",
    "Estelle",
    "Irma",
    "Susie",
    "Eula",
    "Winifred",
    "Eloise",
    "Janice",
    "Maggie",
    "Antoinette",
    "Imogene",
    "Nina",
    "Rosalie",
    "Lorene",
    "Olive",
    "Sadie",
    "Regina",
    "Victoria",
    "Henrietta",
    "Della",
    "Bettie",
    "Lila",
    "Faye",
    "Fern",
    "Johnnie",
    "Jeannette",
];

/// Pick a 1920s nickname deterministically from an AgentId.
///
/// Uses the UUID bytes to index into the combined pool of 400 names
/// (200 boys + 200 girls), so the same agent ID always gets the same nickname.
#[must_use]
pub fn nickname_from_id(id: AgentId) -> String {
    let bytes = id.as_bytes();
    let total = NICKNAMES_1920S_BOYS.len() + NICKNAMES_1920S_GIRLS.len();
    let index = (bytes[0] as usize * 256 + bytes[1] as usize) % total;
    if index < NICKNAMES_1920S_BOYS.len() {
        NICKNAMES_1920S_BOYS[index].to_string()
    } else {
        NICKNAMES_1920S_GIRLS[index - NICKNAMES_1920S_BOYS.len()].to_string()
    }
}

/// A first-class role definition for agent routing and policy.
///
/// Roles are explicit metadata rather than inferred from agent names.
/// They can be defined in config or created programmatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDefinition {
    /// Unique role identifier (e.g., "worker", "reviewer", "researcher").
    pub id: String,
    /// Human-readable description shown to conductor/orchestrator.
    pub description: String,
    /// Developer instructions / behavior constraints.
    #[serde(default)]
    pub instructions: Option<String>,
    /// Optional default CLI to use for agents with this role.
    #[serde(default)]
    pub default_cli: Option<String>,
}

impl RoleDefinition {
    /// Create a new role definition.
    #[must_use]
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            instructions: None,
            default_cli: None,
        }
    }

    /// Set developer instructions.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Set default CLI.
    #[must_use]
    pub fn with_default_cli(mut self, cli: impl Into<String>) -> Self {
        self.default_cli = Some(cli.into());
        self
    }
}

/// Built-in role IDs.
pub mod roles {
    /// Default role for unspecified agents.
    pub const DEFAULT: &str = "default";
    /// General worker role.
    pub const WORKER: &str = "worker";
    /// Code reviewer role.
    pub const REVIEWER: &str = "reviewer";
    /// Research / exploration role.
    pub const RESEARCHER: &str = "researcher";
    /// Task runner / watcher role.
    pub const RUNNER: &str = "runner";
}

/// Return the built-in role definitions.
#[must_use]
pub fn builtin_roles() -> Vec<RoleDefinition> {
    vec![
        RoleDefinition::new(roles::DEFAULT, "Default role for unspecified agents"),
        RoleDefinition::new(roles::WORKER, "General-purpose implementation worker"),
        RoleDefinition::new(roles::REVIEWER, "Code reviewer and auditor"),
        RoleDefinition::new(roles::RESEARCHER, "Research and exploration agent"),
        RoleDefinition::new(roles::RUNNER, "Task runner and CI/deploy watcher"),
    ]
}

/// Agent lifecycle state.
///
/// Follows RAR worker lifecycle: cold → starting → idle → working → draining → stopped
///
/// ```text
///                     work arrives
///         Cold ─────────────────────→ Starting → Idle ⇄ Working
///          ↑                                      │
///          │ idle timeout                         │ graceful shutdown
///          │                                      ↓
///          └──────────── Stopped ←──────────── Draining
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    /// Agent is registered but no process is running (scale-to-zero state).
    /// When work arrives for a Cold agent, the orchestrator needs to start a process.
    Cold,
    /// Agent is starting up
    #[default]
    Starting,
    /// Agent is idle, waiting for work
    Idle,
    /// Agent is working on a task
    Working,
    /// Agent is paused
    Paused,
    /// Agent is finishing its current task but won't accept new work.
    /// Used during graceful shutdown, rolling deploys, or idle timeout.
    /// Transitions to Stopped after current task completes.
    Draining,
    /// Agent has stopped
    Stopped,
    /// Agent encountered an error
    Error,
}

impl AgentState {
    /// Check if agent is in a terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped | Self::Error | Self::Cold)
    }

    /// Check if agent can accept new work.
    #[must_use]
    pub fn can_accept_work(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Check if agent is in an active (process running) state.
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Idle | Self::Working | Self::Paused | Self::Draining
        )
    }

    /// Get emoji representation for display.
    #[must_use]
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Cold => "🧊",
            Self::Starting => "🔄",
            Self::Idle => "💤",
            Self::Working => "⚡",
            Self::Paused => "⏸️",
            Self::Draining => "🔻",
            Self::Stopped => "⏹️",
            Self::Error => "❌",
        }
    }
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cold => write!(f, "Cold"),
            Self::Starting => write!(f, "Starting"),
            Self::Idle => write!(f, "Idle"),
            Self::Working => write!(f, "Working"),
            Self::Paused => write!(f, "Paused"),
            Self::Draining => write!(f, "Draining"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Error => write!(f, "Error"),
        }
    }
}

/// Configuration for an agent CLI (e.g., claude, auggie, codex).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCli {
    /// CLI name (e.g., "claude", "auggie", "codex")
    pub name: String,
    /// Command to run the agent CLI
    pub command: String,
    /// Working directory
    pub workdir: Option<String>,
    /// Environment variables
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

impl AgentCli {
    /// Create a new agent CLI configuration.
    #[must_use]
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            workdir: None,
            env: std::collections::HashMap::new(),
        }
    }
}

/// An agent in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Unique agent identifier (stable machine address)
    pub id: AgentId,
    /// Human-readable name (canonical address for messaging)
    pub name: String,
    /// Optional human-facing nickname (separate from canonical name)
    #[serde(default)]
    pub nickname: Option<String>,
    /// Explicit role ID (e.g., "worker", "reviewer", "researcher").
    /// Used for routing and policy instead of agent-name inference.
    #[serde(default)]
    pub role_id: Option<String>,
    /// Parent agent ID (who spawned/delegated this agent, if any)
    #[serde(default)]
    pub parent_agent_id: Option<AgentId>,
    /// How this agent session was created
    #[serde(default)]
    pub spawn_mode: SpawnMode,
    /// Agent type
    pub agent_type: AgentType,
    /// Current state
    pub state: AgentState,
    /// CLI being used (e.g., "claude", "auggie")
    pub cli: String,
    /// Current task (if working)
    pub current_task: Option<crate::task::TaskId>,
    /// When agent was created
    pub created_at: DateTime<Utc>,
    /// Last heartbeat timestamp
    pub last_heartbeat: DateTime<Utc>,
    /// Number of tasks completed
    pub tasks_completed: u64,
    /// Number of rounds completed
    #[serde(default)]
    pub rounds_completed: u64,
    /// Last time the agent was actively working (for idle timeout detection).
    /// Defaults to created_at if never set.
    #[serde(default = "chrono::Utc::now")]
    pub last_active_at: DateTime<Utc>,
}

impl Agent {
    /// Create a new agent.
    ///
    /// Automatically assigns a 1920s-era nickname based on the agent's UUID,
    /// honoring the founding era of Tiny Town, Colorado.
    #[must_use]
    pub fn new(name: impl Into<String>, cli: impl Into<String>, agent_type: AgentType) -> Self {
        let now = Utc::now();
        let id = AgentId::new();
        let nickname = nickname_from_id(id);
        Self {
            id,
            name: name.into(),
            nickname: Some(nickname),
            role_id: None,
            parent_agent_id: None,
            spawn_mode: SpawnMode::Fresh,
            agent_type,
            state: AgentState::Starting,
            cli: cli.into(),
            current_task: None,
            created_at: now,
            last_heartbeat: now,
            tasks_completed: 0,
            rounds_completed: 0,
            last_active_at: now,
        }
    }

    /// Set the role ID for this agent.
    #[must_use]
    pub fn with_role(mut self, role_id: impl Into<String>) -> Self {
        self.role_id = Some(role_id.into());
        self
    }

    /// Set a human-facing nickname for this agent.
    #[must_use]
    pub fn with_nickname(mut self, nickname: impl Into<String>) -> Self {
        self.nickname = Some(nickname.into());
        self
    }

    /// Set the parent agent ID (who spawned this agent).
    #[must_use]
    pub fn with_parent(mut self, parent_id: AgentId) -> Self {
        self.parent_agent_id = Some(parent_id);
        self
    }

    /// Set the spawn mode.
    #[must_use]
    pub fn with_spawn_mode(mut self, mode: SpawnMode) -> Self {
        self.spawn_mode = mode;
        self
    }

    /// Get the display name: nickname if set, otherwise canonical name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.nickname.as_deref().unwrap_or(&self.name)
    }

    /// Get the standard display label: "Nickname [role]" (e.g., "Fred [reviewer]").
    ///
    /// Falls back to "name [role]" if no nickname is set, and omits the
    /// role bracket when the role is "default".
    #[must_use]
    pub fn display_label(&self) -> String {
        let name_part = self.display_name();
        let role = self.effective_role();
        if role == roles::DEFAULT {
            format!("{} [{}]", name_part, self.name)
        } else {
            format!("{} [{}]", name_part, role)
        }
    }

    /// Get the effective role ID, falling back to "default".
    #[must_use]
    pub fn effective_role(&self) -> &str {
        self.role_id.as_deref().unwrap_or(roles::DEFAULT)
    }

    /// Create a supervisor agent.
    #[must_use]
    pub fn supervisor(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: AgentId::supervisor(),
            name: name.into(),
            nickname: None,
            role_id: None,
            parent_agent_id: None,
            spawn_mode: SpawnMode::Fresh,
            agent_type: AgentType::Supervisor,
            state: AgentState::Starting,
            cli: "supervisor".into(),
            current_task: None,
            created_at: now,
            last_heartbeat: now,
            tasks_completed: 0,
            rounds_completed: 0,
            last_active_at: now,
        }
    }
}
