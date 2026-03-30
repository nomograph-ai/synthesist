package main

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
