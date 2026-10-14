package main

import (
	"encoding/json"
	"fmt"
	"strings"
)

// legacyFamilyRule is the selector contract shared with the production registry compiler.
type legacyFamilyRule struct {
	ID              string   `json:"id"`
	Fact            string   `json:"fact"`
	Members         []string `json:"members"`
	Suffix          string   `json:"suffix"`
	Prefix          string   `json:"prefix"`
	Contains        string   `json:"contains"`
	StateProperty   string   `json:"state_property"`
	ExcludeMembers  []string `json:"exclude_members"`
	ExcludeContains string   `json:"exclude_contains"`
}

var legacyFamilyRules, legacyFamilyRulesError = loadLegacyFamilyRules()

// loadLegacyFamilyRules reads the compiler's single family policy for historical generation.
func loadLegacyFamilyRules() ([]legacyFamilyRule, error) {
	data, err := readAssetData(".", "../../asset-compiler/data/registry-policy.json")
	if err != nil {
		return nil, err
	}
	var policy struct {
		Rules []legacyFamilyRule `json:"model_family_rules"`
	}
	if err := json.Unmarshal(data, &policy); err != nil {
		return nil, err
	}
	if len(policy.Rules) == 0 {
		return nil, fmt.Errorf("missing model family rules")
	}
	return policy.Rules, nil
}

// matches applies the shared identifier and state-property selectors with exclusions first.
func (rule legacyFamilyRule) matches(name string, properties []StateProperty) bool {
	for _, excluded := range rule.ExcludeMembers {
		if name == excluded {
			return false
		}
	}
	if rule.ExcludeContains != "" && strings.Contains(name, rule.ExcludeContains) {
		return false
	}
	for _, member := range rule.Members {
		if member == name {
			return true
		}
	}
	if rule.Suffix != "" && strings.HasSuffix(name, rule.Suffix) {
		return true
	}
	if rule.Prefix != "" && strings.HasPrefix(strings.TrimPrefix(name, "minecraft:"), rule.Prefix) {
		return true
	}
	if rule.Contains != "" && strings.Contains(name, rule.Contains) {
		return true
	}
	for _, property := range properties {
		if property.Name == rule.StateProperty {
			return true
		}
	}
	return false
}

// legacyModelFamily resolves the first shared rule without making a second name catalog.
func legacyModelFamily(state SourceState) (ModelFamily, error) {
	if legacyFamilyRulesError != nil {
		return ModelFamilyUnknown, legacyFamilyRulesError
	}
	for _, rule := range legacyFamilyRules {
		if rule.matches(state.Name, state.Properties) {
			family, ok := modelFamilyByName[strings.TrimPrefix(rule.Fact, "family:")]
			if !ok {
				return ModelFamilyUnknown, fmt.Errorf("unknown family rule %s", rule.ID)
			}
			return family, nil
		}
	}
	return ModelFamilyUnknown, nil
}

// legacyFamilyMember checks a named shared group when its selector decoder needs admission.
func legacyFamilyMember(name, id string) bool {
	if legacyFamilyRulesError != nil {
		return false
	}
	if !strings.Contains(name, ":") {
		name = "minecraft:" + name
	}
	for _, rule := range legacyFamilyRules {
		if rule.ID == id {
			return rule.matches(name, nil)
		}
	}
	return false
}
