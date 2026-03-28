// Package types defines the core data model for Synthesist v5.
// These types ARE the schema. The JSON tags are the wire format.
// LLM agents read and write these via the synthesist CLI.
package types

// Status represents a task's lifecycle state.
type Status string

const (
	StatusPending    Status = "pending"
	StatusInProgress Status = "in_progress"
	StatusDone       Status = "done"
	StatusBlocked    Status = "blocked"
	StatusWaiting    Status = "waiting"
	StatusCancelled  Status = "cancelled"
)

// TaskType distinguishes regular tasks from retrospective nodes.
type TaskType string

const (
	TypeTask  TaskType = "task"
	TypeRetro TaskType = "retro"
)

// Stance is a stakeholder's posture toward a technical direction.
type Stance string

const (
	StanceSupportive Stance = "supportive"
	StanceCautious   Stance = "cautious"
	StanceOpposed    Stance = "opposed"
	StanceNeutral    Stance = "neutral"
	StanceUnknown    Stance = "unknown"
)

// Confidence is the citation tier for an assessment.
type Confidence string

const (
	ConfDocumented  Confidence = "documented"
	ConfVerified    Confidence = "verified"
	ConfInferred    Confidence = "inferred"
	ConfSpeculative Confidence = "speculative"
)

// InfluenceRole describes how a stakeholder relates to a task.
type InfluenceRole string

const (
	RoleMaintainer InfluenceRole = "maintainer"
	RoleReviewer   InfluenceRole = "reviewer"
	RoleApprover   InfluenceRole = "approver"
	RoleBlocker    InfluenceRole = "blocker"
	RoleChampion   InfluenceRole = "champion"
	RoleObserver   InfluenceRole = "observer"
)

// SignalType categorizes where a signal was observed.
type SignalType string

const (
	SignalPRComment     SignalType = "pr_comment"
	SignalIssueComment  SignalType = "issue_comment"
	SignalReview        SignalType = "review"
	SignalCommitMessage SignalType = "commit_message"
	SignalChat          SignalType = "chat"
	SignalMeeting       SignalType = "meeting"
	SignalEmail         SignalType = "email"
	SignalOther         SignalType = "other"
)

// DirectionStatus tracks the lifecycle of an upstream technical direction.
type DirectionStatus string

const (
	DirCommitted    DirectionStatus = "committed"
	DirProposed     DirectionStatus = "proposed"
	DirExperimental DirectionStatus = "experimental"
	DirRejected     DirectionStatus = "rejected"
)

// ArchiveReason explains why a spec was archived.
type ArchiveReason string

const (
	ReasonCompleted  ArchiveReason = "completed"
	ReasonAbandoned  ArchiveReason = "abandoned"
	ReasonSuperseded ArchiveReason = "superseded"
	ReasonDeferred   ArchiveReason = "deferred"
)

// --- Task DAG layer ---

// Criterion is an executable acceptance check.
type Criterion struct {
	Criterion string `json:"criterion"`
	Verify    string `json:"verify"`
}

// Waiter describes an external blocker with a machine-checkable resolution.
type Waiter struct {
	Reason     string  `json:"reason"`
	External   string  `json:"external"`
	Check      string  `json:"check"`
	CheckAfter *string `json:"check_after,omitempty"`
}

// Transform is a labeled move from a retrospective, abstract enough to replay.
type Transform struct {
	Label        string `json:"label"`
	Description  string `json:"description"`
	Transferable bool   `json:"transferable"`
}

// Quality holds review scores and validation history.
type Quality struct {
	Score       *float64     `json:"score"`
	Validations []Validation `json:"validations"`
}

// Validation is a single review entry.
type Validation struct {
	Reviewer      string   `json:"reviewer"`
	Date          string   `json:"date"`
	Score         float64  `json:"score"`
	Findings      string   `json:"findings"`
	TasksReviewed []string `json:"tasks_reviewed"`
}

// Task is a node in the DAG. Regular tasks and retro nodes share this type,
// distinguished by the Type field.
type Task struct {
	ID          string      `json:"id"`
	Type        TaskType    `json:"type"`
	Summary     string      `json:"summary"`
	Description *string     `json:"description"`
	Files       []string    `json:"files"`
	DependsOn   []string    `json:"depends_on"`
	Status      Status      `json:"status"`
	Gate        *string     `json:"gate"`
	Owner       *string     `json:"owner,omitempty"`
	Created     string      `json:"created"`
	Completed   *string     `json:"completed,omitempty"`
	Acceptance  []Criterion `json:"acceptance"`
	Waiter      *Waiter     `json:"waiter,omitempty"`
	FailureNote *string     `json:"failure_note"`

	// Retro-specific fields (type == "retro")
	Arc          *string     `json:"arc,omitempty"`
	Transforms   []Transform `json:"transforms,omitempty"`
	DurationDays *int        `json:"duration_days,omitempty"`
	Patterns     []string    `json:"patterns,omitempty"` // refs to pattern IDs
}

// SpecState is the top-level state.json structure.
type SpecState struct {
	Spec    string  `json:"spec"`
	Tasks   []Task  `json:"tasks"`
	Quality Quality `json:"quality"`
}

// --- Landscape layer ---

// Stakeholder is a human actor relevant to the work.
type Stakeholder struct {
	ID      string   `json:"id"`
	Name    *string  `json:"name,omitempty"`
	Context string   `json:"context"`
	Orgs    []string `json:"orgs,omitempty"`
}

// Disposition is a stakeholder's assessed stance on a technical direction
// at a point in time. The core question: what implementation choices will
// this person accept?
type Disposition struct {
	ID                string     `json:"id"`
	Stakeholder       string     `json:"stakeholder"`
	Topic             string     `json:"topic"`
	Stance            Stance     `json:"stance"`
	PreferredApproach *string    `json:"preferred_approach,omitempty"`
	Detail            *string    `json:"detail,omitempty"`
	Confidence        Confidence `json:"confidence"`
	ValidFrom         string     `json:"valid_from"`
	ValidUntil        *string    `json:"valid_until,omitempty"`
	SupersededBy      *string    `json:"superseded_by,omitempty"`
}

// Signal is observable evidence from a stakeholder. Immutable once recorded.
// Bi-temporal: Date is when the signal occurred (event time), RecordedDate
// is when we captured it (transaction time). This matters for retroactive
// discovery -- reading a 2-week-old PR comment today.
type Signal struct {
	ID             string     `json:"id"`
	Stakeholder    string     `json:"stakeholder"`
	Date           string     `json:"date"`
	RecordedDate   string     `json:"recorded_date"`
	Source         string     `json:"source"`
	SourceType     SignalType `json:"source_type"`
	Content        string     `json:"content"`
	Interpretation *string    `json:"interpretation,omitempty"`
	OurAction      *string    `json:"our_action,omitempty"` // what we did that prompted this signal
}

// Influence describes how a stakeholder relates to a specific task.
type Influence struct {
	Stakeholder string        `json:"stakeholder"`
	Task        string        `json:"task"`
	Role        InfluenceRole `json:"role"`
}

// Landscape is the per-spec stakeholder intelligence file.
type Landscape struct {
	Spec         string        `json:"spec"`
	Stakeholders []string      `json:"stakeholders"`
	Dispositions []Disposition `json:"dispositions"`
	Signals      []Signal      `json:"signals"`
	Influences   []Influence   `json:"influences"`
}

// StakeholderRegistry is the per-tree stakeholder identity registry.
type StakeholderRegistry struct {
	Tree         string        `json:"tree"`
	Stakeholders []Stakeholder `json:"stakeholders"`
}

// --- Pattern + Retro layer ---

// Pattern is a named, reusable approach discovered through work.
type Pattern struct {
	ID              string   `json:"id"`
	Name            string   `json:"name"`
	Description     string   `json:"description"`
	Transferability *string  `json:"transferability,omitempty"`
	FirstObserved   string   `json:"first_observed"`
	ObservedIn      []string `json:"observed_in"`
}

// PatternRegistry is the per-tree pattern registry.
type PatternRegistry struct {
	Tree     string    `json:"tree"`
	Patterns []Pattern `json:"patterns"`
}

// --- Direction layer ---

// Direction tracks an upstream technical trajectory. Per-tree, not per-spec.
// A direction like "migrate from REST v2 to v3" affects multiple specs.
// When status changes (proposed -> committed), create a new Direction and
// supersede the old one -- same temporal model as dispositions.
// Directions with status=committed are "positions" (settled upstream decisions).
type Direction struct {
	ID           string          `json:"id"`
	Project      string          `json:"project"`            // "upstream-org/auth-service"
	Topic        string          `json:"topic"`              // "API versioning strategy"
	Status       DirectionStatus `json:"status"`             // committed = position (settled)
	Owner        *string         `json:"owner,omitempty"`    // stakeholder ref
	Timeline     *string         `json:"timeline,omitempty"` // "6-12 months"
	Detail       *string         `json:"detail,omitempty"`
	Impact       string          `json:"impact"` // what this means for our work
	References   []string        `json:"references,omitempty"`
	ValidFrom    string          `json:"valid_from"`
	ValidUntil   *string         `json:"valid_until,omitempty"`
	SupersededBy *string         `json:"superseded_by,omitempty"`
}

// DirectionImpact links a direction to a spec it affects.
type DirectionImpact struct {
	DirectionID  string `json:"direction_id"`
	AffectedTree string `json:"affected_tree"`
	AffectedSpec string `json:"affected_spec"`
	Description  string `json:"description"` // how this direction affects this spec
}

// TaskProvenance records "while doing X we discovered we need Y."
// Causal edge in the task graph (MAGMA causal layer).
type TaskProvenance struct {
	SourceTree string `json:"source_tree"`
	SourceSpec string `json:"source_spec"`
	SourceTask string `json:"source_task"`
	TargetTree string `json:"target_tree"`
	TargetSpec string `json:"target_spec"`
	TargetTask string `json:"target_task"`
	Note       string `json:"note,omitempty"`
}

// --- Spec layer (intent and institutional memory) ---

// Spec represents a unit of work's intent, constraints, and decisions.
// Tasks live in the task DAG; this captures the "why" and "what rules."
type Spec struct {
	Tree        string  `json:"tree"`
	ID          string  `json:"id"`
	Goal        *string `json:"goal,omitempty"`
	Constraints *string `json:"constraints,omitempty"`
	Decisions   *string `json:"decisions,omitempty"`
	Created     *string `json:"created,omitempty"`
}

// PropagationLink models "when this spec's output changes, the target
// spec needs updates." Ordered by seq for cascade direction.
type PropagationLink struct {
	SourceTree  string  `json:"source_tree"`
	SourceSpec  string  `json:"source_spec"`
	TargetTree  string  `json:"target_tree"`
	TargetSpec  string  `json:"target_spec"`
	Seq         int     `json:"seq"`
	Description *string `json:"description,omitempty"`
}

// Discovery is a timestamped, append-only finding made during work.
type Discovery struct {
	Tree    string  `json:"tree"`
	Spec    string  `json:"spec"`
	ID      string  `json:"id"`
	Date    string  `json:"date"`
	Author  *string `json:"author,omitempty"`
	Finding string  `json:"finding"`
	Impact  *string `json:"impact,omitempty"`
	Action  *string `json:"action,omitempty"`
}

// --- Estate layer ---

// TreeInfo describes a context tree in the estate.
type TreeInfo struct {
	Path        string `json:"path"`
	Status      string `json:"status"`
	Description string `json:"description"`
}

// Thread is an active workstream in the estate.
type Thread struct {
	ID      string  `json:"id"`
	Tree    string  `json:"tree"`
	Spec    *string `json:"spec"`
	Task    *string `json:"task"`
	Date    string  `json:"date"`
	Summary string  `json:"summary"`
	Waiter  *Waiter `json:"waiter,omitempty"`
}

// Estate is the top-level navigation switchboard.
type Estate struct {
	Version       int                 `json:"version"`
	Trees         map[string]TreeInfo `json:"trees"`
	ActiveThreads []Thread            `json:"active_threads"`
}

// --- Campaign layer ---

// ActiveSpec is a spec in a campaign's active list.
type ActiveSpec struct {
	ID        string   `json:"id"`
	Path      string   `json:"path"`
	Summary   string   `json:"summary"`
	Phase     *string  `json:"phase"`
	BlockedBy []string `json:"blocked_by"`
}

// BacklogItem is a spec in a campaign's backlog.
type BacklogItem struct {
	ID        string   `json:"id"`
	Title     string   `json:"title"`
	Summary   string   `json:"summary"`
	BlockedBy []string `json:"blocked_by"`
	Path      *string  `json:"path"`
}

// Campaign is the tree-level coordination file.
type Campaign struct {
	Tree        string        `json:"tree"`
	Description string        `json:"description"`
	SubTrees    []string      `json:"sub_trees,omitempty"`
	Active      []ActiveSpec  `json:"active"`
	Backlog     []BacklogItem `json:"backlog"`
}

// --- Archive layer ---

// ArchivedSpec is a completed/deferred spec record.
type ArchivedSpec struct {
	ID            string        `json:"id"`
	Path          *string       `json:"path"`
	Summary       string        `json:"summary"`
	Archived      string        `json:"archived"`
	Reason        ArchiveReason `json:"reason"`
	Outcome       *string       `json:"outcome,omitempty"`
	DurationDays  *int          `json:"duration_days,omitempty"`
	Patterns      []string      `json:"patterns,omitempty"`
	Contributions []string      `json:"contributions,omitempty"`
}

// Archive is the tree-level archive file.
type Archive struct {
	Tree     string         `json:"tree"`
	Archived []ArchivedSpec `json:"archived"`
}

// --- Config ---

// Config holds synthesist runtime configuration.
type Config struct {
	AutoCommit    bool   `json:"auto_commit"`
	CommitTrailer string `json:"commit_trailer"`
	DefaultAuthor string `json:"default_author"`
}

// DefaultConfig returns the default configuration.
func DefaultConfig() Config {
	return Config{
		AutoCommit:    true,
		CommitTrailer: "AI-Assisted: yes",
		DefaultAuthor: "synthesist",
	}
}
