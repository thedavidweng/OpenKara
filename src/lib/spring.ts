interface SpringConfig {
  stiffness: number;
  damping: number;
  mass: number;
  precision: number;
}

const DEFAULT_CONFIG: SpringConfig = {
  stiffness: 180,
  damping: 12,
  mass: 1,
  precision: 0.001,
};

export class Spring {
  private position: number;
  private velocity: number;
  private target: number;
  private config: SpringConfig;
  private settled: boolean;

  constructor(initialValue = 0, config: Partial<SpringConfig> = {}) {
    this.position = initialValue;
    this.velocity = 0;
    this.target = initialValue;
    this.config = { ...DEFAULT_CONFIG, ...config };
    this.settled = true;
  }

  setTarget(target: number): boolean {
    if (this.target === target) return false;
    this.target = target;
    this.settled = false;
    return true;
  }

  update(dt: number) {
    if (this.settled) return;

    const { stiffness, damping, mass, precision } = this.config;

    const displacement = this.position - this.target;
    const springForce = -stiffness * displacement;
    const dampingForce = -damping * this.velocity;
    const acceleration = (springForce + dampingForce) / mass;

    this.velocity += acceleration * dt;
    this.position += this.velocity * dt;

    if (
      Math.abs(this.velocity) < precision &&
      Math.abs(this.position - this.target) < precision
    ) {
      this.position = this.target;
      this.velocity = 0;
      this.settled = true;
    }
  }

  getPosition(): number {
    return this.position;
  }

  getVelocity(): number {
    return this.velocity;
  }

  isSettled(): boolean {
    return this.settled;
  }

  jumpTo(value: number) {
    this.position = value;
    this.target = value;
    this.velocity = 0;
    this.settled = true;
  }

  syncPosition(position: number) {
    this.position = position;
    this.settled = false;
  }

  setVelocity(velocity: number) {
    this.velocity = velocity;
    if (velocity !== 0) {
      this.settled = false;
    }
  }
}
