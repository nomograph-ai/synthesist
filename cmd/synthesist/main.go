package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/alecthomas/kong"
	"gitlab.com/nomograph/synthesist/internal/store"
)

var version = "dev" // set by -ldflags "-X main.version=..."

// noCommit disables auto-commit when --no-commit is passed globally.
var noCommit bool

// discoverStore wraps store.Discover, applies flags, and ensures session branch.
func discoverStore() (*store.Store, error) {
	s, err := store.Discover()
	if err != nil {
		return nil, err
	}
	if noCommit {
		s.AutoCommit = false
	}
	if err := s.EnsureSession(); err != nil {
		_ = s.Close()
		return nil, err
	}
	return s, nil
}

// --- CLI struct tree ---

type CLI struct {
	// Estate
	Init     InitCmd     `cmd:"" help:"Scaffold estate structure in current directory"`
	Scaffold ScaffoldCmd `cmd:"" help:"Bootstrap synthesist in a project (CLAUDE.md, .mise.toml, init)"`
	Status   StatusCmd   `cmd:"" help:"Show estate overview"`
	Check    CheckCmd    `cmd:"" help:"Validate referential integrity"`

	// Estate management
	Tree      TreeCmd      `cmd:"" help:"Manage trees"`
	Thread    ThreadCmd    `cmd:"" help:"Manage threads"`
	Campaign  CampaignCmd  `cmd:"" help:"Manage campaigns"`
	Archive   ArchiveCmd   `cmd:"" help:"Manage archives"`
	Discovery DiscoveryCmd `cmd:"" help:"Record findings"`

	// Task DAG
	Task TaskCmd `cmd:"" help:"Manage task DAGs"`

	// Landscape
	Stakeholder StakeholderCmd `cmd:"" help:"Manage stakeholders"`
	Disposition DispositionCmd `cmd:"" help:"Manage dispositions"`
	Signal      SignalCmd      `cmd:"" help:"Manage signals"`
	Direction   DirectionCmd   `cmd:"" help:"Manage directions"`

	// Specs + Propagation
	Spec        SpecCmd        `cmd:"" help:"Manage specs"`
	Propagation PropagationCmd `cmd:"" help:"Manage propagation chains"`

	// Retro + Patterns
	Retro   RetroCmd   `cmd:"" help:"Manage retrospectives"`
	Pattern PatternCmd `cmd:"" help:"Manage patterns"`

	// Query
	Ready     ReadyCmd     `cmd:"" help:"Show unblocked pending tasks"`
	Landscape LandscapeCmd `cmd:"" help:"Show landscape"`
	Stance    StanceCmd    `cmd:"" help:"Stakeholder dispositions"`
	Replay    ReplayCmd    `cmd:"" help:"Generate replay playbook"`

	// Phase
	Phase PhaseCmd `cmd:"" help:"Manage workflow phase"`

	// Sessions
	Session SessionCmd `cmd:"" help:"Manage concurrent sessions"`

	// Database
	Migrate MigrateCmd `cmd:"" help:"Check or run database migrations"`
	Export  ExportCmd  `cmd:"" help:"Export all tables as JSON"`
	Import  ImportCmd  `cmd:"" help:"Import tables from JSON (export format)"`

	// Meta
	Skill   SkillCmd   `cmd:"" help:"Output synthesist skill file"`
	Version VersionCmd `cmd:"" help:"Print version"`
}

// --- Simple top-level commands ---

type InitCmd struct{}

func (c *InitCmd) Run() error { return cmdInit() }

type StatusCmd struct{}

func (c *StatusCmd) Run() error { return cmdStatus() }

type CheckCmd struct{}

func (c *CheckCmd) Run() error { return cmdCheck() }

type SkillCmd struct{}

func (c *SkillCmd) Run() error {
	fmt.Print(skillContent)
	return nil
}

type ScaffoldCmd struct{}

func (c *ScaffoldCmd) Run() error { return cmdScaffold() }

type ExportCmd struct{}

func (c *ExportCmd) Run() error { return cmdExport() }

type ImportCmd struct {
	File string `arg:"" optional:"" help:"JSON file to import (stdin if omitted)"`
}

func (c *ImportCmd) Run() error { return cmdImport(c) }

type MigrateCmd struct{}

func (c *MigrateCmd) Run() error { return cmdMigrate() }

type VersionCmd struct{}

func (c *VersionCmd) Run() error {
	fmt.Println(version)
	return nil
}

// --- Tree ---

type TreeCmd struct {
	Create TreeCreateCmd `cmd:"" help:"Create a tree"`
	List   TreeListCmd   `cmd:"" help:"List all trees"`
}

type TreeCreateCmd struct {
	Name        string `arg:"" help:"Tree name"`
	Description string `name:"description" default:"" help:"Tree description"`
	Status      string `name:"status" default:"active" help:"Tree status"`
}

func (c *TreeCreateCmd) Run() error { return cmdTreeCreate(c) }

type TreeListCmd struct{}

func (c *TreeListCmd) Run() error { return cmdTreeList() }

// --- Thread ---

type ThreadCmd struct {
	Create ThreadCreateCmd `cmd:"" help:"Create a thread"`
	List   ThreadListCmd   `cmd:"" help:"List threads"`
}

type ThreadCreateCmd struct {
	ID      string `arg:"" help:"Thread ID"`
	Tree    string `name:"tree" required:"" help:"Tree name"`
	Summary string `name:"summary" required:"" help:"Thread summary"`
	Spec    string `name:"spec" default:"" help:"Spec ID"`
	Task    string `name:"task" default:"" help:"Task ID"`
	Date    string `name:"date" default:"" help:"Date YYYY-MM-DD"`
}

func (c *ThreadCreateCmd) Run() error { return cmdThreadCreate(c) }

type ThreadListCmd struct{}

func (c *ThreadListCmd) Run() error { return cmdThreadList() }

// --- Campaign ---

type CampaignCmd struct {
	Active  CampaignActiveCmd  `cmd:"" help:"Add spec to campaign active list"`
	Backlog CampaignBacklogCmd `cmd:"" help:"Add spec to campaign backlog"`
	List    CampaignListCmd    `cmd:"" help:"List campaign specs"`
}

type CampaignActiveCmd struct {
	Tree      string `arg:"" help:"Tree name"`
	SpecID    string `arg:"" help:"Spec ID"`
	Summary   string `name:"summary" default:"" help:"Summary"`
	Phase     string `name:"phase" default:"" help:"Phase"`
	BlockedBy string `name:"blocked-by" default:"" help:"Comma-separated blocked-by spec IDs"`
}

func (c *CampaignActiveCmd) Run() error { return cmdCampaignActive(c) }

type CampaignBacklogCmd struct {
	Tree      string `arg:"" help:"Tree name"`
	SpecID    string `arg:"" help:"Spec ID"`
	Title     string `name:"title" default:"" help:"Title"`
	Summary   string `name:"summary" default:"" help:"Summary"`
	BlockedBy string `name:"blocked-by" default:"" help:"Comma-separated blocked-by spec IDs"`
}

func (c *CampaignBacklogCmd) Run() error { return cmdCampaignBacklog(c) }

type CampaignListCmd struct {
	Tree string `arg:"" help:"Tree name"`
}

func (c *CampaignListCmd) Run() error { return cmdCampaignList(c) }

// --- Archive ---

type ArchiveCmd struct {
	Add  ArchiveAddCmd  `cmd:"" help:"Archive a spec"`
	List ArchiveListCmd `cmd:"" help:"List archived specs"`
}

type ArchiveAddCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
	Reason   string `name:"reason" required:"" help:"Archive reason (completed, abandoned, superseded, deferred)"`
	Outcome  string `name:"outcome" default:"" help:"Outcome description"`
	Archived string `name:"archived" default:"" help:"Archive date YYYY-MM-DD"`
	Patterns string `name:"patterns" default:"" help:"Comma-separated pattern IDs"`
}

func (c *ArchiveAddCmd) Run() error { return cmdArchiveAdd(c) }

type ArchiveListCmd struct {
	Tree string `arg:"" help:"Tree name"`
}

func (c *ArchiveListCmd) Run() error { return cmdArchiveList(c) }

// --- Discovery ---

type DiscoveryCmd struct {
	Add  DiscoveryAddCmd  `cmd:"" help:"Record a finding"`
	List DiscoveryListCmd `cmd:"" help:"List discoveries"`
}

type DiscoveryAddCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
	Finding  string `name:"finding" required:"" help:"Finding description"`
	Impact   string `name:"impact" default:"" help:"Impact description"`
	Action   string `name:"action" default:"" help:"Action taken"`
	Author   string `name:"author" default:"" help:"Author name"`
	Date     string `name:"date" default:"" help:"Date YYYY-MM-DD"`
}

func (c *DiscoveryAddCmd) Run() error { return cmdDiscoveryAdd(c) }

type DiscoveryListCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *DiscoveryListCmd) Run() error { return cmdDiscoveryList(c) }

// --- Task ---

type TaskCmd struct {
	Create     TaskCreateCmd     `cmd:"" help:"Create a task"`
	List       TaskListCmd       `cmd:"" help:"List tasks"`
	Claim      TaskClaimCmd      `cmd:"" help:"Claim a task"`
	Done       TaskDoneCmd       `cmd:"" help:"Complete a task"`
	Wait       TaskWaitCmd       `cmd:"" help:"Set waiting status"`
	Block      TaskBlockCmd      `cmd:"" help:"Set blocked status"`
	Ready      TaskReadyCmd      `cmd:"" help:"Show ready tasks"`
	Acceptance TaskAcceptanceCmd `cmd:"" help:"Add acceptance criterion"`
	Cancel     TaskCancelCmd     `cmd:"" help:"Cancel a task"`
}

type TaskCreateCmd struct {
	TreeSpec  string `arg:"" help:"tree/spec format"`
	Summary   string `arg:"" help:"Task summary"`
	DependsOn string `name:"depends-on" default:"" help:"Comma-separated dependency IDs"`
	Gate      string `name:"gate" default:"" help:"Gate type (human)"`
	Files     string `name:"files" default:"" help:"Comma-separated file paths"`
	Status    string `name:"status" default:"" help:"Initial status"`
	ID        string `name:"id" default:"" help:"Task ID"`
	Created   string `name:"created" default:"" help:"Creation date YYYY-MM-DD"`
	Completed string `name:"completed" default:"" help:"Completion date YYYY-MM-DD"`
}

func (c *TaskCreateCmd) Run() error { return cmdTaskCreate(c) }

type TaskListCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
	Human    bool   `name:"human" help:"Human-readable output"`
	Active   bool   `name:"active" help:"Hide cancelled tasks"`
}

func (c *TaskListCmd) Run() error { return cmdTaskList(c) }

type TaskClaimCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
	TaskID   string `arg:"" help:"Task ID"`
}

func (c *TaskClaimCmd) Run() error { return cmdTaskClaim(c) }

type TaskDoneCmd struct {
	TreeSpec   string `arg:"" help:"tree/spec format"`
	TaskID     string `arg:"" help:"Task ID"`
	SkipVerify bool   `name:"skip-verify" help:"Skip acceptance criteria verification"`
}

func (c *TaskDoneCmd) Run() error { return cmdTaskDone(c) }

type TaskWaitCmd struct {
	TreeSpec   string `arg:"" help:"tree/spec format"`
	TaskID     string `arg:"" help:"Task ID"`
	Reason     string `name:"reason" required:"" help:"Wait reason"`
	External   string `name:"external" required:"" help:"External URL"`
	Check      string `name:"check" required:"" help:"Check command"`
	CheckAfter string `name:"check-after" default:"" help:"Check after date YYYY-MM-DD"`
}

func (c *TaskWaitCmd) Run() error { return cmdTaskWait(c) }

type TaskBlockCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
	TaskID   string `arg:"" help:"Task ID"`
}

func (c *TaskBlockCmd) Run() error { return cmdTaskBlock(c) }

type TaskReadyCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *TaskReadyCmd) Run() error { return cmdTaskReady(c) }

type TaskAcceptanceCmd struct {
	TreeSpec  string `arg:"" help:"tree/spec format"`
	TaskID    string `arg:"" help:"Task ID"`
	Criterion string `name:"criterion" required:"" help:"Acceptance criterion"`
	Verify    string `name:"verify" required:"" help:"Verify command"`
}

func (c *TaskAcceptanceCmd) Run() error { return cmdTaskAcceptance(c) }

type TaskCancelCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
	TaskID   string `arg:"" help:"Task ID"`
	Reason   string `name:"reason" default:"" help:"Cancellation reason"`
}

func (c *TaskCancelCmd) Run() error { return cmdTaskCancel(c) }

// --- Stakeholder ---

type StakeholderCmd struct {
	Add  StakeholderAddCmd  `cmd:"" help:"Register a stakeholder"`
	List StakeholderListCmd `cmd:"" help:"List stakeholders"`
}

type StakeholderAddCmd struct {
	Tree    string `arg:"" help:"Tree name"`
	ID      string `arg:"" help:"Stakeholder ID"`
	Context string `name:"context" required:"" help:"Stakeholder role/context"`
	Name    string `name:"name" default:"" help:"Full name"`
	Orgs    string `name:"orgs" default:"" help:"Comma-separated organizations"`
}

func (c *StakeholderAddCmd) Run() error { return cmdStakeholderAdd(c) }

type StakeholderListCmd struct {
	Tree string `arg:"" help:"Tree name"`
}

func (c *StakeholderListCmd) Run() error { return cmdStakeholderList(c) }

// --- Disposition ---

type DispositionCmd struct {
	Add       DispositionAddCmd       `cmd:"" help:"Record a disposition"`
	List      DispositionListCmd      `cmd:"" help:"List dispositions"`
	Supersede DispositionSupersedeCmd `cmd:"" help:"Supersede a disposition"`
}

type DispositionAddCmd struct {
	TreeSpec      string `arg:"" help:"tree/spec format"`
	StakeholderID string `arg:"" name:"stakeholder" help:"Stakeholder ID"`
	Topic         string `name:"topic" required:"" help:"Topic"`
	Stance        string `name:"stance" required:"" help:"Stance (supportive, cautious, opposed, neutral, unknown)"`
	Confidence    string `name:"confidence" required:"" help:"Confidence (documented, verified, inferred, speculative)"`
	Preferred     string `name:"preferred" default:"" help:"Preferred approach"`
}

func (c *DispositionAddCmd) Run() error { return cmdDispositionAdd(c) }

type DispositionListCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *DispositionListCmd) Run() error { return cmdDispositionList(c) }

type DispositionSupersedeCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
	OldID    string `arg:"" name:"disposition-id" help:"Disposition ID to supersede"`
	NewStance string `name:"new-stance" required:"" help:"New stance"`
	Preferred string `name:"preferred" default:"" help:"New preferred approach"`
	Evidence  string `name:"evidence" default:"" help:"Signal ID as evidence"`
}

func (c *DispositionSupersedeCmd) Run() error { return cmdDispositionSupersede(c) }

// --- Signal ---

type SignalCmd struct {
	Record SignalRecordCmd `cmd:"" help:"Record an observed signal"`
	List   SignalListCmd   `cmd:"" help:"List signals"`
}

type SignalRecordCmd struct {
	TreeSpec       string `arg:"" help:"tree/spec format"`
	StakeholderID  string `arg:"" name:"stakeholder" help:"Stakeholder ID"`
	Source         string `name:"source" required:"" help:"Source URL"`
	Type           string `name:"type" required:"" help:"Source type (pr_comment, issue_comment, review, etc.)"`
	Content        string `name:"content" required:"" help:"Signal content"`
	Date           string `name:"date" default:"" help:"Date YYYY-MM-DD"`
	OurAction      string `name:"our-action" default:"" help:"Our action in response"`
	Interpretation string `name:"interpretation" default:"" help:"Interpretation of signal"`
}

func (c *SignalRecordCmd) Run() error { return cmdSignalRecord(c) }

type SignalListCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *SignalListCmd) Run() error { return cmdSignalList(c) }

// --- Direction ---

type DirectionCmd struct {
	Add    DirectionAddCmd    `cmd:"" help:"Record a technical direction"`
	List   DirectionListCmd   `cmd:"" help:"List directions"`
	Impact DirectionImpactCmd `cmd:"" help:"Link direction to affected spec"`
}

type DirectionAddCmd struct {
	Tree     string `arg:"" help:"Tree name"`
	Project  string `name:"project" required:"" help:"Project (org/repo)"`
	Topic    string `name:"topic" required:"" help:"Topic"`
	Status   string `name:"status" required:"" help:"Status (committed, proposed, experimental, rejected)"`
	Impact   string `name:"impact" required:"" help:"Impact description"`
	Owner    string `name:"owner" default:"" help:"Owner stakeholder ID"`
	Timeline string `name:"timeline" default:"" help:"Timeline"`
}

func (c *DirectionAddCmd) Run() error { return cmdDirectionAdd(c) }

type DirectionListCmd struct {
	Tree string `arg:"" help:"Tree name"`
}

func (c *DirectionListCmd) Run() error { return cmdDirectionList(c) }

type DirectionImpactCmd struct {
	Tree         string `arg:"" help:"Tree name"`
	DirectionID  string `arg:"" name:"direction-id" help:"Direction ID"`
	AffectedTree string `name:"affected-tree" required:"" help:"Affected tree"`
	AffectedSpec string `name:"affected-spec" required:"" help:"Affected spec"`
	Description  string `name:"description" required:"" help:"Impact description"`
}

func (c *DirectionImpactCmd) Run() error { return cmdDirectionImpact(c) }

// --- Spec ---

type SpecCmd struct {
	Create SpecCreateCmd `cmd:"" help:"Create a spec"`
	Show   SpecShowCmd   `cmd:"" help:"Show spec details"`
	Update SpecUpdateCmd `cmd:"" help:"Update a spec"`
}

type SpecCreateCmd struct {
	TreeSpec    string `arg:"" help:"tree/spec format"`
	Goal        string `name:"goal" default:"" help:"Spec goal"`
	Constraints string `name:"constraints" default:"" help:"Spec constraints"`
	Decisions   string `name:"decisions" default:"" help:"Spec decisions"`
}

func (c *SpecCreateCmd) Run() error { return cmdSpecCreate(c) }

type SpecShowCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *SpecShowCmd) Run() error { return cmdSpecShow(c) }

type SpecUpdateCmd struct {
	TreeSpec    string `arg:"" help:"tree/spec format"`
	Goal        string `name:"goal" default:"" help:"New goal"`
	Constraints string `name:"constraints" default:"" help:"New constraints"`
	Decisions   string `name:"decisions" default:"" help:"New decisions"`
}

func (c *SpecUpdateCmd) Run() error { return cmdSpecUpdate(c) }

// --- Propagation ---

type PropagationCmd struct {
	Add   PropagationAddCmd   `cmd:"" help:"Link specs for change propagation"`
	List  PropagationListCmd  `cmd:"" help:"Show propagation links"`
	Check PropagationCheckCmd `cmd:"" help:"Find stale downstream specs"`
}

type PropagationAddCmd struct {
	Source      string `arg:"" help:"Source tree/spec"`
	Target      string `arg:"" help:"Target tree/spec"`
	Seq         int    `name:"seq" default:"0" help:"Sequence number"`
	Description string `name:"description" default:"" help:"Link description"`
}

func (c *PropagationAddCmd) Run() error { return cmdPropagationAdd(c) }

type PropagationListCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *PropagationListCmd) Run() error { return cmdPropagationList(c) }

type PropagationCheckCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *PropagationCheckCmd) Run() error { return cmdPropagationCheck(c) }

// --- Retro ---

type RetroCmd struct {
	Create    RetroCreateCmd    `cmd:"" help:"Create a retrospective"`
	Show      RetroShowCmd      `cmd:"" help:"Show retrospective"`
	Transform RetroTransformCmd `cmd:"" help:"Add a transform"`
}

type RetroCreateCmd struct {
	TreeSpec  string `arg:"" help:"tree/spec format"`
	Arc       string `name:"arc" required:"" help:"Arc narrative"`
	DependsOn string `name:"depends-on" default:"" help:"Comma-separated dependency task IDs"`
}

func (c *RetroCreateCmd) Run() error { return cmdRetroCreate(c) }

type RetroShowCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *RetroShowCmd) Run() error { return cmdRetroShow(c) }

type RetroTransformCmd struct {
	TreeSpec     string `arg:"" help:"tree/spec format"`
	Label        string `name:"label" required:"" help:"Transform label"`
	Description  string `name:"description" required:"" help:"Transform description"`
	Transferable bool   `name:"transferable" help:"Mark as transferable"`
}

func (c *RetroTransformCmd) Run() error { return cmdRetroTransform(c) }

// --- Pattern ---

type PatternCmd struct {
	Register PatternRegisterCmd `cmd:"" help:"Register a pattern"`
	List     PatternListCmd     `cmd:"" help:"List patterns"`
}

type PatternRegisterCmd struct {
	Tree            string `arg:"" help:"Tree name"`
	ID              string `arg:"" help:"Pattern ID"`
	Name            string `name:"name" required:"" help:"Pattern name"`
	Description     string `name:"description" required:"" help:"Pattern description"`
	Transferability string `name:"transferability" default:"" help:"Transferability note"`
	ObservedIn      string `name:"observed-in" default:"" help:"Comma-separated spec IDs"`
}

func (c *PatternRegisterCmd) Run() error { return cmdPatternRegister(c) }

type PatternListCmd struct {
	Tree string `arg:"" help:"Tree name"`
}

func (c *PatternListCmd) Run() error { return cmdPatternList(c) }

// --- Query commands ---

type ReadyCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *ReadyCmd) Run() error { return cmdTaskReady(&TaskReadyCmd{TreeSpec: c.TreeSpec}) }

type LandscapeCmd struct {
	Show LandscapeShowCmd `cmd:"" help:"Show landscape for a spec"`
}

type LandscapeShowCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *LandscapeShowCmd) Run() error { return cmdLandscapeShow(c) }

type StanceCmd struct {
	StakeholderID string `arg:"" help:"Stakeholder ID"`
	Topic         string `arg:"" optional:"" help:"Optional topic filter"`
}

func (c *StanceCmd) Run() error { return cmdStance(c) }

type ReplayCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *ReplayCmd) Run() error { return cmdReplay(c) }

// --- Phase ---

type PhaseCmd struct {
	Set  PhaseSetCmd  `cmd:"" help:"Set the current phase"`
	Show PhaseShowCmd `cmd:"" help:"Show the current phase"`
}

type PhaseSetCmd struct {
	Name string `arg:"" help:"Phase name (orient, plan, agree, execute, reflect, replan, report)"`
}

func (c *PhaseSetCmd) Run() error { return cmdPhaseSet(c) }

type PhaseShowCmd struct{}

func (c *PhaseShowCmd) Run() error { return cmdPhaseShow() }

// --- Session ---

type SessionCmd struct {
	Start  SessionStartCmd  `cmd:"" help:"Start a session"`
	Merge  SessionMergeCmd  `cmd:"" help:"Merge a session"`
	List   SessionListCmd   `cmd:"" help:"List sessions"`
	Status SessionStatusCmd `cmd:"" help:"Show session status"`
	Prune  SessionPruneCmd  `cmd:"" help:"Prune stale sessions"`
}

type SessionStartCmd struct {
	SessionID string `arg:"" help:"Session ID"`
	Spec      string `name:"spec" default:"" help:"Advisory spec lock hint"`
}

func (c *SessionStartCmd) Run() error { return cmdSessionStart(c) }

type SessionMergeCmd struct {
	SessionID string `arg:"" help:"Session ID"`
	Ours      bool   `name:"ours" help:"Resolve conflicts with ours"`
	Theirs    bool   `name:"theirs" help:"Resolve conflicts with theirs"`
}

func (c *SessionMergeCmd) Run() error { return cmdSessionMerge(c) }

type SessionListCmd struct{}

func (c *SessionListCmd) Run() error { return cmdSessionList() }

type SessionStatusCmd struct {
	SessionID string `arg:"" help:"Session ID"`
}

func (c *SessionStatusCmd) Run() error { return cmdSessionStatus(c) }

type SessionPruneCmd struct {
	Hours int `name:"hours" default:"168" help:"Prune sessions inactive for more than N hours"`
}

func (c *SessionPruneCmd) Run() error { return cmdSessionPrune(c) }

// --- Main ---

func main() {
	// Strip global flags from os.Args before kong parses.
	// These can appear anywhere in the arg list.
	var filtered []string
	var forcePhase bool
	for _, arg := range os.Args {
		switch {
		case arg == "--no-commit":
			noCommit = true
		case arg == "--force":
			forcePhase = true
		case len(arg) > 10 && arg[:10] == "--session=":
			store.Session = arg[10:]
		default:
			filtered = append(filtered, arg)
		}
	}
	// Also check SYNTHESIST_SESSION env var (flag takes precedence)
	if store.Session == "" {
		store.Session = os.Getenv("SYNTHESIST_SESSION")
	}
	os.Args = filtered

	var cli CLI
	ctx := kong.Parse(&cli,
		kong.Name("synthesist"),
		kong.Description("specification graph manager"),
		kong.UsageOnError(),
	)

	// Generate skill content from the kong struct.
	initSkillContent(cli)

	// Enforce session for write operations.
	// Read-only commands and read subcommands work without a session.
	// This mirrors the original enforcement: top-level read-only commands
	// and read-only subcommands (list, show) bypass the session requirement.
	readOnlyCommands := map[string]bool{
		"init": true, "scaffold": true, "status": true, "check": true,
		"ready": true, "landscape": true, "stance": true, "replay": true,
		"session": true, "skill": true, "version": true, "help": true,
		"migrate": true, "export": true,
	}
	readOnlySubcommands := map[string]bool{
		"list": true, "show": true,
	}

	cmdPath := ctx.Command()
	parts := strings.Fields(cmdPath)
	topCmd := ""
	subCmd := ""
	if len(parts) > 0 {
		topCmd = parts[0]
	}
	if len(parts) > 1 {
		subCmd = parts[1]
	}

	if !readOnlyCommands[topCmd] && !readOnlySubcommands[subCmd] && store.Session == "" {
		_, _ = fmt.Fprintf(os.Stderr, "error: --session or SYNTHESIST_SESSION required for write operations\n")
		_, _ = fmt.Fprintf(os.Stderr, "  start a session:  synthesist session start <session-id>\n")
		_, _ = fmt.Fprintf(os.Stderr, "  then:             synthesist --session=<id> %s\n", cmdPath)
		os.Exit(1)
	}

	// Phase enforcement: check if the operation is allowed in the current phase.
	// Only enforced for write operations (reads always allowed).
	// --force (stripped from os.Args above) bypasses enforcement.
	if !readOnlyCommands[topCmd] && !readOnlySubcommands[subCmd] && !forcePhase && topCmd != "phase" {
		if s, err := store.Discover(); err == nil {
			var phase string
			if qErr := s.DB.QueryRow("SELECT name FROM phase WHERE id = 1").Scan(&phase); qErr == nil {
				violation := ""
				switch phase {
				case "orient":
					violation = "no writes allowed in ORIENT phase"
				case "plan":
					if topCmd == "task" && (subCmd == "claim" || subCmd == "done" || subCmd == "block") {
						violation = "cannot claim/complete tasks in PLAN phase — transition to EXECUTE first"
					}
				case "agree":
					violation = "no operations in AGREE phase — present the plan and wait for human approval"
				case "execute":
					if topCmd == "task" && subCmd == "create" {
						violation = "cannot create tasks in EXECUTE phase — transition to REPLAN first"
					}
					if topCmd == "task" && subCmd == "cancel" {
						violation = "cannot cancel tasks in EXECUTE phase — transition to REPLAN first"
					}
					if topCmd == "spec" && subCmd == "create" {
						violation = "cannot create specs in EXECUTE phase — transition to REPLAN first"
					}
				case "report":
					violation = "no writes allowed in REPORT phase"
				}
				if violation != "" {
					_, _ = fmt.Fprintf(os.Stderr, "error: phase violation (%s): %s\n", phase, violation)
					_, _ = fmt.Fprintf(os.Stderr, "  current phase: %s\n", phase)
					_, _ = fmt.Fprintf(os.Stderr, "  use --force to override\n")
					_ = s.Close()
					os.Exit(1)
				}
			}
			_ = s.Close()
		}
	}

	err := ctx.Run()
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}
