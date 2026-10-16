package main

import (
	"fmt"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/player/form"
	"github.com/df-mc/dragonfly/server/world"
)

// init exposes original form fixtures for client rendering and submission comparisons.
func init() {
	cmd.Register(cmd.New("formfixture", "Opens a form interaction fixture", nil, formFixture{}))
}

type formFixture struct {
	Kind cmd.Optional[string] `cmd:"kind"`
}

// Run presents each wire form family through Dragonfly's standard form owner.
func (f formFixture) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("Only players can open forms")
		return
	}
	switch f.Kind.LoadOr("custom") {
	case "custom":
		p.SendForm(form.New(customFixture{
			Label:    form.NewLabel("Change each control, then submit."),
			Toggle:   form.NewToggle("Enabled", false),
			Input:    form.NewInput("Name", "initial", "Enter a name"),
			Slider:   form.NewSlider("Amount", 0, 10, 2, 4),
			Dropdown: form.NewDropdown("Choice", []string{"First", "Second", "Third"}, 1),
			Step:     form.NewStepSlider("Step", []string{"Low", "Medium", "High"}, 0),
		}, "Form fixture"))
	case "simple":
		p.SendForm(form.NewMenu(buttonFixture{First: form.NewButton("First", ""), Second: form.NewButton("Second", "")}, "Menu fixture").WithBody("Select either button."))
	case "modal":
		p.SendForm(form.NewModal(buttonFixture{First: form.NewButton("Accept", ""), Second: form.NewButton("Decline", "")}, "Modal fixture").WithBody("Select either response."))
	default:
		o.Error("Use custom, simple or modal")
	}
}

type customFixture struct {
	Label    form.Label
	Toggle   form.Toggle
	Input    form.Input
	Slider   form.Slider
	Dropdown form.Dropdown
	Step     form.StepSlider
}

// Submit reports the values actually decoded by the server, including the focused input.
func (f customFixture) Submit(s form.Submitter, _ *world.Tx) {
	s.(*player.Player).Message(fmt.Sprintf("FORM RESULT enabled=%t name=%q amount=%g choice=%d step=%d", f.Toggle.Value(), f.Input.Value(), f.Slider.Value(), f.Dropdown.Value(), f.Step.Value()))
}

// Close distinguishes dismissal from a submitted value.
func (customFixture) Close(s form.Submitter, _ *world.Tx) {
	s.(*player.Player).Message("FORM CANCELLED")
}

type buttonFixture struct {
	First  form.Button
	Second form.Button
}

// Submit reports the wire button selection in both the menu and modal fixtures.
func (f buttonFixture) Submit(s form.Submitter, pressed form.Button, _ *world.Tx) {
	index := 0
	if pressed == f.Second {
		index = 1
	}
	s.(*player.Player).Message(fmt.Sprintf("FORM BUTTON %d", index))
}

// Close reports explicit menu or modal dismissal.
func (buttonFixture) Close(s form.Submitter, _ *world.Tx) {
	s.(*player.Player).Message("FORM CANCELLED")
}
