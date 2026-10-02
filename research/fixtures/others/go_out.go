package main

import "fmt"

type Player struct {
	Health int
	Name   string
}

func (p *Player) Update(dt int) {
	p.Health += dt
}

func (p *Player) GetHealth() int {
	return p.Health
}

func NewPlayer(health int) *Player {
	return &Player{Health: health}
}

func main() {
	p := NewPlayer(100)
	p.Update(1)
	fmt.Println(p.GetHealth())
}
