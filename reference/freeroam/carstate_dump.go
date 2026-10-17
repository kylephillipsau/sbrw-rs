// Decodes car states with freeroam's own carstate package, for
// carstate_vectors.py: one hex car state per stdin line, one JSON object per
// stdout line. Floats are printed as their IEEE 754 bits so the Rust test can
// compare them exactly. Copied into the freeroam module as cmd/carstatedump.
package main

import (
	"bufio"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"os"

	"github.com/WorldUnitedNFS/freeroam/carstate"
)

func bits(v float64) string {
	return fmt.Sprintf("%016x", math.Float64bits(v))
}

func vec(v struct{ X, Y, Z float64 }) []string {
	return []string{bits(v.X), bits(v.Y), bits(v.Z)}
}

func main() {
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 1<<20), 1<<20)
	out := json.NewEncoder(os.Stdout)
	for in.Scan() {
		data, err := hex.DecodeString(in.Text())
		if err != nil {
			panic(err)
		}
		row := map[string]interface{}{"packet": in.Text()}
		pkt, err := carstate.NewPacketReader(data).Decode()
		if err != nil {
			row["error"] = err.Error()
			out.Encode(row)
			continue
		}
		row["sim_time"] = pkt.SimTime()
		row["ground"] = pkt.OnGround()
		row["pos"] = vec(pkt.Coordinates())
		row["lin_vel"] = vec(pkt.LinearVelocity())
		row["ang_vel"] = vec(pkt.AngularVelocity())
		switch p := pkt.(type) {
		case *carstate.GroundPacket:
			q := p.OrientationQuaternion
			row["orientation"] = []string{bits(q.X), bits(q.Y), bits(q.Z), bits(q.W)}
			row["wheels"] = []string{bits(p.FrontWheelsDirection), bits(p.RearWheelsDirection)}
			row["effects"] = p.ActiveEffectFlags
		case *carstate.AirPacket:
			row["yaw_pitch_roll"] = []string{bits(p.Yaw), bits(p.Pitch), bits(p.Roll)}
		}
		out.Encode(row)
	}
}
