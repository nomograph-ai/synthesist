package main

// skillContent is generated at startup from the kong CLI struct
// and the embedded state machine document.
var skillContent string

func initSkillContent(cli CLI) {
	skillContent = generateSkillContent(cli)
}
