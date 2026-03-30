package main

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
	Detail        string `name:"detail" default:"" help:"Reasoning or context for this assessment"`
	Evidence      string `name:"evidence" default:"" help:"Signal ID as evidence"`
}

func (c *DispositionAddCmd) Run() error { return cmdDispositionAdd(c) }

type DispositionListCmd struct {
	TreeSpec string `arg:"" help:"tree/spec format"`
}

func (c *DispositionListCmd) Run() error { return cmdDispositionList(c) }

type DispositionSupersedeCmd struct {
	TreeSpec  string `arg:"" help:"tree/spec format"`
	OldID     string `arg:"" name:"disposition-id" help:"Disposition ID to supersede"`
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
