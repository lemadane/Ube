// Range counters use i64 internally: adding any two i32 values cannot wrap.
// Only values inside the inclusive i32 bounds are exposed to the loop body.
struct RangeState {
    counter: Register,
    end: IrOperand,
    step: IrOperand,
}

impl IrBuilder {
    fn range_binary(&mut self, op: BinaryOp, left: IrOperand, right: IrOperand, result_ty: Type) -> IrOperand {
        let operand_ty = left.get_type();
        let dest = self.fresh_reg();
        self.emit(IrInstruction::Binary { dest, op, left, right, operand_ty, result_ty: result_ty.clone() });
        IrOperand::Register(dest, result_ty)
    }

    fn range_load(&mut self, state: &RangeState) -> IrOperand {
        let dest = self.fresh_reg();
        self.emit(IrInstruction::Load { dest, ptr: state.counter, ty: Type::Long });
        IrOperand::Register(dest, Type::Long)
    }

    fn build_range_state(&mut self, expression: &Expr) -> Option<RangeState> {
        let ExprKind::RangeIterator { start, end, step } = &expression.kind else { return None; };
        let start_value = self.build_expr(start);
        let start_value = self.cast(start_value, Type::Long);
        let end_value = self.build_expr(end);
        let end_value = self.cast(end_value, Type::Long);
        let step_value = self.build_expr(step);
        let step_value = self.cast(step_value, Type::Long);
        let zero = self.range_binary(BinaryOp::Equal, step_value.clone(), IrOperand::ConstantLong(0), Type::Bool);
        let failed = self.fresh_block();
        let ready = self.fresh_block();
        self.emit(IrInstruction::Branch { cond: zero, then_block: failed, else_block: ready });
        self.end_block();
        self.current_block_id = Some(failed);
        let error = self.make_exception(IrOperand::ConstantString("Range step must not be zero".into()), step.span);
        self.route_exception(error);
        self.end_block();
        self.current_block_id = Some(ready);
        let counter = self.fresh_reg();
        self.emit(IrInstruction::Alloca { dest: counter, ty: Type::Long });
        self.emit(IrInstruction::Store { val: start_value, ptr: counter });
        Some(RangeState { counter, end: end_value, step: step_value })
    }

    fn range_condition(&mut self, state: &RangeState) -> IrOperand {
        let current = self.range_load(state);
        let ascending = self.range_binary(BinaryOp::Greater, state.step.clone(), IrOperand::ConstantLong(0), Type::Bool);
        let descending = self.range_binary(BinaryOp::Less, state.step.clone(), IrOperand::ConstantLong(0), Type::Bool);
        let below = self.range_binary(BinaryOp::LessEqual, current.clone(), state.end.clone(), Type::Bool);
        let above = self.range_binary(BinaryOp::GreaterEqual, current, state.end.clone(), Type::Bool);
        let forward = self.range_binary(BinaryOp::And, ascending, below, Type::Bool);
        let backward = self.range_binary(BinaryOp::And, descending, above, Type::Bool);
        self.range_binary(BinaryOp::Or, forward, backward, Type::Bool)
    }

    fn range_advance(&mut self, state: &RangeState) {
        let current = self.range_load(state);
        let next = self.range_binary(BinaryOp::Add, current, state.step.clone(), Type::Long);
        self.emit(IrInstruction::Store { val: next, ptr: state.counter });
    }
}
