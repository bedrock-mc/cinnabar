package main

import (
	"encoding/json"
	"fmt"
	"go/ast"
	"go/constant"
	"go/parser"
	"go/token"
	"go/types"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"

	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

type idSource struct {
	File   string `json:"file"`
	Prefix string `json:"prefix"`
	Type   string `json:"type"`
}

type idFixture struct {
	Sources    []idSource                  `json:"sources"`
	Groups     map[string]map[string]int64 `json:"groups"`
	SlotCounts map[string]int              `json:"slot_counts"`
}

// protocolIDFixture evaluates the pinned module's constant declarations with Go's type checker.
func protocolIDFixture() (idFixture, error) {
	function := runtime.FuncForPC(reflect.ValueOf(protocol.NewEntityMetadata).Pointer())
	file, _ := function.FileLine(function.Entry())
	root := filepath.Dir(file)
	sources := []idSource{
		{"entity_metadata.go", "EntityDataKey", "ActorDataKey"},
		{"entity_metadata.go", "EntityDataFlag", "ActorFlag"},
		{"packet/mob_effect.go", "Effect", "Effect"},
		{"packet/level_event.go", "LevelEvent", "LevelEvent"},
		{"container.go", "ContainerType", "WindowType"},
		{"container.go", "Container", "ContainerName"},
		{"inventory.go", "WindowID", "WindowID"},
		{"command.go", "CommandArgType", "CommandArgType"},
		{"command.go", "CommandArg", "CommandArgFlag"},
	}
	fixture := idFixture{Sources: sources, Groups: make(map[string]map[string]int64)}
	for _, source := range sources {
		values, err := sourceConstants(filepath.Join(root, source.File), source.Prefix)
		if err != nil {
			return fixture, err
		}
		fixture.Groups[source.Type] = values
	}
	fixture.SlotCounts = make(map[string]int)
	for name, raw := range fixture.Groups["WindowType"] {
		if raw < 0 {
			continue
		}
		if count, fixed := protocol.ContainerTypeSlotCount(byte(raw)); fixed {
			fixture.SlotCounts[name] = count
		}
	}
	return fixture, nil
}

// sourceConstants retains complete const blocks so blanks, aliases and iota keep their meaning.
func sourceConstants(path, prefix string) (map[string]int64, error) {
	set := token.NewFileSet()
	file, err := parser.ParseFile(set, path, nil, 0)
	if err != nil {
		return nil, err
	}
	declarations := make([]ast.Decl, 0)
	for _, declaration := range file.Decls {
		block, ok := declaration.(*ast.GenDecl)
		if !ok || block.Tok != token.CONST {
			continue
		}
		for _, spec := range block.Specs {
			found := false
			for _, name := range spec.(*ast.ValueSpec).Names {
				found = found || strings.HasPrefix(name.Name, prefix)
			}
			if found {
				declarations = append(declarations, block)
				break
			}
		}
	}
	file.Decls = declarations
	file.Imports = nil
	config := types.Config{}
	pkg, err := config.Check("fixture", set, []*ast.File{file}, nil)
	if err != nil {
		return nil, fmt.Errorf("evaluate %s: %w", path, err)
	}
	values := make(map[string]int64)
	for _, name := range pkg.Scope().Names() {
		if !strings.HasPrefix(name, prefix) ||
			(prefix == "Container" && strings.HasPrefix(name, "ContainerType")) ||
			(prefix == "CommandArg" && strings.HasPrefix(name, "CommandArgType")) {
			continue
		}
		value := pkg.Scope().Lookup(name).(*types.Const)
		raw, ok := constant.Int64Val(value.Val())
		if !ok {
			return nil, fmt.Errorf("%s is not an int64 constant", name)
		}
		values[strings.TrimPrefix(name, prefix)] = raw
	}
	if len(values) == 0 {
		return nil, fmt.Errorf("no %s constants in %s", prefix, path)
	}
	return values, nil
}

// writeProtocolIDs writes a deterministic numeric fixture without copying upstream source.
func writeProtocolIDs(path string) error {
	fixture, err := protocolIDFixture()
	if err != nil {
		return err
	}
	encoded, err := json.MarshalIndent(fixture, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, append(encoded, '\n'), 0o644)
}
