package main

import (
	"errors"
	"fmt"

	vanilla "github.com/bedrock-mc/vanilla-gen"
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
)

type worldGenerators map[world.Dimension]vanilla.Generator

func (s settings) configureGenerators(conf *server.Config) (worldGenerators, error) {
	generators := make(worldGenerators)
	fixture := s.terrainFixture || s.opaqueOverdraw
	if s.generator != "normal" || fixture {
		return generators, nil
	}
	for _, dim := range []world.Dimension{world.Overworld, world.Nether, world.End} {
		generator, err := vanilla.NewForDimensionWithConfig(s.seed, dim, vanilla.GeneratorConfig{
			Acceleration: vanilla.AccelerationConfig{Mode: vanilla.AccelerationAuto},
		})
		if err != nil {
			return nil, errors.Join(fmt.Errorf("configure world generator: %w", err), generators.close())
		}
		generators[dim] = generator
	}
	conf.Generator = func(dim world.Dimension) world.Generator { return generators[dim] }
	return generators, nil
}

func (generators worldGenerators) close() error {
	var err error
	for _, generator := range generators {
		err = errors.Join(err, generator.Close())
	}
	return err
}
